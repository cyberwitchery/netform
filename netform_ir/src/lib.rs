//! lossless intermediate representation (IR) for network device configuration text.
//!
//! this crate provides:
//! - a tree model (`Document`, `Node`, `LineNode`, `BlockNode`)
//! - a conservative parser (`parse_generic`, `parse_with_dialect`)
//! - a lossless renderer (`Document::render`)
//!
//! the parser is intentionally conservative:
//! - it only uses indentation as a structural cue
//! - unknown patterns are preserved as regular lines
//! - no input lines are dropped
//!
//! # Example
//!
//! ```rust
//! use netform_ir::parse_generic;
//!
//! let input = "interface Ethernet1\n  description uplink\n";
//! let doc = parse_generic(input);
//! assert_eq!(doc.render(), input);
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Range;

/// stable arena identifier for a node in a [`Document`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub usize);

/// location path used by diffs and diagnostics (`root_index`, then child indices).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Path(pub Vec<usize>);

/// source span pointing to a single line and byte range in the original input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// minimal tokenized representation of a content line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedLineParts {
    pub head: String,
    pub args: Vec<String>,
}

/// lightweight classification used by parser, normalization, and diff views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriviaKind {
    Blank,
    Comment,
    Content,
    Unknown,
    /// free-text body of a multi-line literal region (see [`Dialect::literal_region`]):
    /// never tokenized or key-hinted, inert for block structure, compared verbatim.
    Literal,
}

/// leaf node preserving original raw text and parse metadata for one line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineNode {
    pub raw: String,
    pub line_ending: String,
    pub span: Span,
    pub parsed: Option<ParsedLineParts>,
    pub key_hint: Option<String>,
    pub trivia: TriviaKind,
}

/// structured block node with a header line and nested children.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockNode {
    pub header: LineNode,
    pub children: Vec<NodeId>,
    pub footer: Option<LineNode>,
    pub kind_label: Option<String>,
}

/// arena node variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Node {
    Line(LineNode),
    Block(BlockNode),
}

/// document metadata attached during parsing.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub source_name: Option<String>,
    pub dialect_hint: DialectHint,
    pub original_bytes: usize,
    pub line_count: usize,
    pub parse_findings: Vec<ParseFinding>,
}

/// declared parser dialect used for this document.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DialectHint {
    #[default]
    Generic,
    Unknown,
    Named(String),
}

/// parser-level uncertainty note attached to a source span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseFinding {
    pub code: String,
    pub message: String,
    pub span: Span,
}

/// lossless parsed document backed by an arena and root node list.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Document {
    pub metadata: DocumentMetadata,
    pub roots: Vec<NodeId>,
    pub arena: Vec<Node>,
}

impl Document {
    /// create an empty document with caller-supplied metadata.
    pub fn new(metadata: DocumentMetadata) -> Self {
        Self {
            metadata,
            roots: Vec::new(),
            arena: Vec::new(),
        }
    }

    /// insert a node and register it as a root.
    pub fn insert_root(&mut self, node: Node) -> NodeId {
        let id = self.insert_node(node);
        self.roots.push(id);
        id
    }

    /// insert a node into the arena and return its stable [`NodeId`].
    pub fn insert_node(&mut self, node: Node) -> NodeId {
        let id = NodeId(self.arena.len());
        self.arena.push(node);
        id
    }

    /// borrow a node by id.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.arena.get(id.0)
    }

    /// append `child` to `parent` if parent is a block.
    ///
    /// returns `true` when attached, `false` when `parent` is not a block.
    pub fn add_child(&mut self, parent: NodeId, child: NodeId) -> bool {
        match self.arena.get_mut(parent.0) {
            Some(Node::Block(block)) => {
                block.children.push(child);
                true
            }
            _ => false,
        }
    }

    /// render the document as exact original line bytes.
    ///
    /// for documents created with this crate's parser, this guarantees lossless
    /// round-trip text rendering.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for root in &self.roots {
            self.render_node(*root, &mut out);
        }
        out
    }

    fn render_node(&self, id: NodeId, out: &mut String) {
        if let Some(node) = self.arena.get(id.0) {
            match node {
                Node::Line(line) => {
                    out.push_str(&line.raw);
                    out.push_str(&line.line_ending);
                }
                Node::Block(block) => {
                    out.push_str(&block.header.raw);
                    out.push_str(&block.header.line_ending);
                    for child in &block.children {
                        self.render_node(*child, out);
                    }
                    if let Some(footer) = &block.footer {
                        out.push_str(&footer.raw);
                        out.push_str(&footer.line_ending);
                    }
                }
            }
        }
    }
}

/// parse input using the built-in generic dialect.
pub fn parse_generic(input: &str) -> Document {
    parse_with_dialect(input, &GenericDialect)
}

/// dialect extension point for trivia classification and line tokenization.
pub trait Dialect {
    /// report a dialect hint to store in [`DocumentMetadata`].
    fn dialect_hint(&self) -> DialectHint {
        DialectHint::Unknown
    }
    /// classify a raw line into trivia/content buckets.
    fn classify_trivia(&self, raw: &str) -> TriviaKind;
    /// optionally tokenize a raw content line into `head` + `args`.
    fn parse_parts(&self, raw: &str) -> Option<ParsedLineParts>;
    /// optionally derive a stable identity hint for this line.
    fn key_hint(
        &self,
        _raw: &str,
        _parsed: Option<&ParsedLineParts>,
        _trivia: TriviaKind,
    ) -> Option<String> {
        None
    }
    /// report whether a raw line is a block-closing terminator for this dialect.
    ///
    /// delimiter-terminated dialects (FortiOS `end`/`next`, Junos `}`/`};`)
    /// return `true` so the parser attaches the terminator to the block it
    /// closes as that [`BlockNode`]'s footer instead of leaving it a detached
    /// sibling.  indentation-only dialects (IOS XE, EOS, NX-OS) keep the default
    /// `false` and are unaffected.
    fn block_terminator(&self, _raw: &str) -> bool {
        false
    }

    /// report whether `opener` opens a block that `terminator`, the next content
    /// or comment line after it and at its indent, closes.
    ///
    /// only asked when `terminator` is a [`Dialect::block_terminator`] line.
    /// returning `true` parses the pair as an empty [`BlockNode`] with
    /// `terminator` as its footer, as Junos `ge-0/0/0 {` and `}` or FortiOS
    /// `edit 1` and `next`; the default `false` keeps them two sibling lines.
    /// it also matches a [`Dialect::block_opener`] line with a later terminator.
    fn empty_block(&self, _opener: &str, _terminator: &str) -> bool {
        false
    }

    /// report whether a raw line opens a block that only its matching
    /// terminator closes, however the lines between them are indented.
    ///
    /// such openers and [`Dialect::block_terminator`] lines nest like brackets:
    /// a terminator matches the innermost opener still open when they share an
    /// indent and [`Dialect::empty_block`] pairs them, as FortiOS `config vdom`
    /// and `end` around unindented `edit` entries.  any other terminator leaves
    /// every opener still open unmatched, and unmatched lines nest by
    /// indentation.  the default `false` leaves all nesting to indentation.
    fn block_opener(&self, _raw: &str) -> bool {
        false
    }

    /// report whether a raw line opens a multi-line literal region, and what
    /// closes it.
    ///
    /// lines after the opener up to and including the terminator become
    /// [`TriviaKind::Literal`]; the opener itself stays [`TriviaKind::Content`].
    /// the returned pattern must be non-empty.
    fn literal_region(&self, _raw: &str) -> Option<LiteralTerminator> {
        None
    }

    /// report whether a raw line opens a multi-line comment region, and what
    /// closes it.
    ///
    /// lines after the opener up to and including the terminator become
    /// [`TriviaKind::Comment`], so `--ignore-comments` still drops them while
    /// nothing inside is tokenized, key-hinted, or able to open a literal
    /// region.  a comment region wins over a literal region opened on the same
    /// line.
    fn comment_region(&self, _raw: &str) -> Option<LiteralTerminator> {
        None
    }
}

/// what closes a multi-line literal region opened by [`Dialect::literal_region`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiteralTerminator {
    /// closed by the first later line containing this text, matching how IOS
    /// ends a `banner <type> <delimiter>` at the next delimiter occurrence.
    Contains(String),
    /// closed by the first later line whose trimmed text equals this, matching
    /// the delimiter-less EOS `banner <type>` … `EOF` form.
    ExactLine(String),
    /// closed by the first later line holding a double quote that is not
    /// backslash-escaped, matching how FortiOS ends a multi-line
    /// `set <field> "…"` value.
    UnescapedQuote,
}

impl LiteralTerminator {
    /// report whether `raw` closes this region.
    pub fn terminates(&self, raw: &str) -> bool {
        match self {
            Self::Contains(pattern) => raw.contains(pattern.as_str()),
            Self::ExactLine(pattern) => raw.trim() == pattern,
            Self::UnescapedQuote => unescaped_quote_count(raw) > 0,
        }
    }

    /// the text this terminator matches on, for diagnostics.
    pub fn pattern(&self) -> &str {
        match self {
            Self::Contains(pattern) | Self::ExactLine(pattern) => pattern,
            Self::UnescapedQuote => "\"",
        }
    }
}

/// report whether `raw` ends with a double-quoted value still open.
///
/// a backslash escapes the character after it, so an inner `\"` neither opens
/// nor closes a value and `\\` is a literal backslash.
///
/// # Example
///
/// ```rust
/// use netform_ir::ends_inside_quoted_value;
///
/// assert!(ends_inside_quoted_value("set buffer \"<a href=\\\"x\\\">"));
/// assert!(!ends_inside_quoted_value("set hostname \"FortiGate\""));
/// ```
pub fn ends_inside_quoted_value(raw: &str) -> bool {
    unescaped_quote_count(raw) % 2 == 1
}

/// byte ranges of the double-quoted values in `raw`, quotes included.
///
/// escapes follow [`ends_inside_quoted_value`]; a value still open at the end
/// of the line runs to the end of `raw`.
///
/// # Example
///
/// ```rust
/// use netform_ir::quoted_spans;
///
/// let spans: Vec<_> = quoted_spans("set a \"x \\\" y\" b \"open").collect();
/// assert_eq!(spans, [6..14, 17..22]);
/// ```
pub fn quoted_spans(raw: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut quotes = unescaped_quotes(raw);
    std::iter::from_fn(move || {
        let open = quotes.next()?;
        Some(open..quotes.next().map_or(raw.len(), |close| close + 1))
    })
}

/// count the double quotes in `raw` that are not backslash-escaped.
fn unescaped_quote_count(raw: &str) -> usize {
    unescaped_quotes(raw).count()
}

/// byte offsets of the double quotes in `raw` that are not backslash-escaped.
fn unescaped_quotes(raw: &str) -> impl Iterator<Item = usize> + '_ {
    let mut escaped = false;

    // no UTF-8 continuation byte can equal `\` or `"`, so bytes suffice.
    raw.bytes().enumerate().filter_map(move |(idx, byte)| {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return Some(idx);
        }
        None
    })
}

/// conservative default dialect for vendor-agnostic parsing.
#[derive(Debug, Default, Clone, Copy)]
pub struct GenericDialect;

impl Dialect for GenericDialect {
    fn dialect_hint(&self) -> DialectHint {
        DialectHint::Generic
    }

    fn classify_trivia(&self, raw: &str) -> TriviaKind {
        classify_trivia(raw)
    }

    fn parse_parts(&self, raw: &str) -> Option<ParsedLineParts> {
        parse_parts(raw)
    }
}

/// parse input into a lossless IR using the given dialect implementation.
///
/// parsing is indentation-based and conservative:
/// - open a block when the next content or comment line is more indented
/// - open an empty block when the next one is a terminator at the same indent
///   that [`Dialect::empty_block`] pairs with the line
/// - close blocks when a content or comment line dedents
/// - blank lines and the bodies of literal and comment regions do neither
/// - a [`Dialect::block_opener`] line with a matching terminator opens a block
///   that only that terminator closes
/// - preserve all lines even when structure is uncertain
pub fn parse_with_dialect<D: Dialect>(input: &str, dialect: &D) -> Document {
    let mut doc = Document::new(DocumentMetadata {
        source_name: None,
        dialect_hint: dialect.dialect_hint(),
        original_bytes: input.len(),
        line_count: 0,
        parse_findings: Vec::new(),
    });

    let lines = collect_lines(
        input,
        dialect,
        &mut doc.metadata.line_count,
        &mut doc.metadata.parse_findings,
    );
    // (indent, block, closed only by its matching terminator)
    let mut parent_stack: Vec<(usize, NodeId, bool)> = Vec::new();
    let partner = match_block_openers(&lines, dialect);

    // pre-compute the index of the next structural line for each position in a
    // single reverse pass (O(n)), avoiding a per-line forward scan that would be
    // O(n²) on large configs.
    let mut next_structural: Vec<Option<usize>> = vec![None; lines.len()];
    {
        let mut last_structural: Option<usize> = None;
        for i in (0..lines.len()).rev() {
            next_structural[i] = last_structural;
            if lines[i].is_structural() {
                last_structural = Some(i);
            }
        }
    }

    // pre-compute block-opening decisions using the O(n) lookup table.
    let opens_block: Vec<bool> = (0..lines.len())
        .map(|idx| {
            if let Some(partner) = partner[idx] {
                return partner > idx;
            }
            let line = &lines[idx];
            line.trivia == TriviaKind::Content
                && next_structural[idx].is_some_and(|next| {
                    let next = &lines[next];
                    next.indent > line.indent
                        || (next.indent == line.indent
                            && next.trivia == TriviaKind::Content
                            && dialect.block_terminator(&next.raw)
                            && dialect.empty_block(&line.raw, &next.raw))
                })
        })
        .collect();

    // pre-compute terminator lines (see `block_terminator`); a line that
    // itself opens a block is never one.
    let is_terminator: Vec<bool> = (0..lines.len())
        .map(|idx| {
            lines[idx].trivia == TriviaKind::Content
                && !opens_block[idx]
                && dialect.block_terminator(&lines[idx].raw)
        })
        .collect();

    for (idx, line) in lines.into_iter().enumerate() {
        if line.trivia == TriviaKind::Content && line.indent > 0 && parent_stack.is_empty() {
            doc.metadata.parse_findings.push(ParseFinding {
                code: "orphan-indentation".to_string(),
                message: "indented content line without an open parent block; line kept as-is"
                    .to_string(),
                span: line.span.clone(),
            });
        }

        // a matched terminator closes its opener's block; other structural lines close on a dedent.
        let mut closed_block: Option<NodeId> = None;
        if partner[idx].is_some_and(|opener| opener < idx) {
            while let Some((_, parent_id, matched)) = parent_stack.pop() {
                closed_block = Some(parent_id);
                if matched {
                    break;
                }
            }
        } else if line.is_structural() {
            while let Some((parent_indent, parent_id, matched)) = parent_stack.last().copied() {
                if !matched && line.indent <= parent_indent {
                    closed_block = Some(parent_id);
                    parent_stack.pop();
                } else {
                    break;
                }
            }
        }

        // attach the terminator as the closed block's footer; the renderer
        // emits the footer after the children, preserving the round trip.
        if is_terminator[idx]
            && let Some(block_id) = closed_block
            && let Some(Node::Block(block)) = doc.arena.get_mut(block_id.0)
            && block.footer.is_none()
        {
            block.footer = Some(line.into_line_node());
            continue;
        }

        let indent = line.indent;

        if opens_block[idx] {
            let block = Node::Block(BlockNode {
                header: line.into_line_node(),
                children: Vec::new(),
                footer: None,
                kind_label: None,
            });
            let id = doc.insert_node(block);
            attach_node(&mut doc, &parent_stack, id);
            parent_stack.push((indent, id, partner[idx].is_some()));
        } else {
            let id = doc.insert_node(Node::Line(line.into_line_node()));
            attach_node(&mut doc, &parent_stack, id);
        }
    }

    doc
}

/// keyword nesting deeper than this is malformed, and its openers are placed by indentation.
const MAX_KEYWORD_DEPTH: usize = 64;

/// map each [`Dialect::block_opener`] line to the terminator that matches it,
/// and that terminator back to it, in one stack pass.
fn match_block_openers<D: Dialect>(lines: &[LineCandidate], dialect: &D) -> Vec<Option<usize>> {
    let mut partner = vec![None; lines.len()];
    let mut open: Vec<usize> = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if line.trivia != TriviaKind::Content {
            continue;
        }
        if dialect.block_terminator(&line.raw) {
            match open.pop() {
                Some(opener)
                    if lines[opener].indent == line.indent
                        && dialect.empty_block(&lines[opener].raw, &line.raw) =>
                {
                    partner[opener] = Some(idx);
                    partner[idx] = Some(opener);
                }
                _ => open.clear(),
            }
        } else if dialect.block_opener(&line.raw) {
            if open.len() == MAX_KEYWORD_DEPTH {
                open.clear();
            }
            open.push(idx);
        }
    }
    partner
}

#[derive(Debug)]
struct LineCandidate {
    raw: String,
    line_ending: String,
    span: Span,
    parsed: Option<ParsedLineParts>,
    key_hint: Option<String>,
    trivia: TriviaKind,
    indent: usize,
    /// inside a dialect region, so inert for block structure.
    in_region: bool,
}

impl LineCandidate {
    /// whether the line's indentation decides block structure.
    fn is_structural(&self) -> bool {
        !self.in_region && !matches!(self.trivia, TriviaKind::Blank | TriviaKind::Literal)
    }

    fn into_line_node(self) -> LineNode {
        LineNode {
            raw: self.raw,
            line_ending: self.line_ending,
            span: self.span,
            parsed: self.parsed,
            key_hint: self.key_hint,
            trivia: self.trivia,
        }
    }
}

/// one physical source line, before dialect classification.
struct RawLine<'a> {
    raw: &'a str,
    line_ending: &'a str,
    span: Span,
}

fn collect_lines<D: Dialect>(
    input: &str,
    dialect: &D,
    line_count: &mut usize,
    parse_findings: &mut Vec<ParseFinding>,
) -> Vec<LineCandidate> {
    let raw_lines = split_raw_lines(input);
    *line_count = raw_lines.len();

    let regions = mark_regions(&raw_lines, dialect, parse_findings);

    raw_lines
        .into_iter()
        .zip(regions)
        .map(|(line, region)| {
            let RawLine {
                raw,
                line_ending,
                span,
            } = line;

            let trivia = match region {
                Some(RegionKind::Literal) => TriviaKind::Literal,
                Some(RegionKind::Comment) => TriviaKind::Comment,
                None => dialect.classify_trivia(raw),
            };
            let parsed = if trivia == TriviaKind::Content {
                dialect.parse_parts(raw)
            } else {
                None
            };
            let key_hint = dialect.key_hint(raw, parsed.as_ref(), trivia);

            if region.is_none() && has_mixed_leading_whitespace(raw) {
                parse_findings.push(ParseFinding {
                    code: "mixed-leading-whitespace".to_string(),
                    message: "line indentation mixes spaces and tabs; structure may be ambiguous"
                        .to_string(),
                    span: span.clone(),
                });
            }

            LineCandidate {
                raw: raw.to_string(),
                line_ending: line_ending.to_string(),
                span,
                parsed,
                key_hint,
                trivia,
                indent: count_indent(raw),
                in_region: region.is_some(),
            }
        })
        .collect()
}

fn split_raw_lines(input: &str) -> Vec<RawLine<'_>> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut line_no = 1usize;

    while start < input.len() {
        let next_lf = input[start..].find('\n').map(|idx| start + idx);
        let (segment, next_start) = if let Some(lf_idx) = next_lf {
            (&input[start..=lf_idx], lf_idx + 1)
        } else {
            (&input[start..], input.len())
        };

        let (raw, line_ending) = split_line_ending(segment);
        out.push(RawLine {
            raw,
            line_ending,
            span: Span {
                line: line_no,
                start_byte: start,
                // spans currently cover the content bytes only (not trailing newline bytes).
                end_byte: start + raw.len(),
            },
        });

        line_no += 1;
        start = next_start;
    }

    out
}

/// what a dialect region makes of the lines inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegionKind {
    Literal,
    Comment,
}

/// mark every line that falls inside a dialect region with that region's kind.
///
/// an opener whose terminator never appears is not entered; it gets an
/// `unterminated-literal-region` or `unterminated-comment-region` finding
/// instead.
fn mark_regions<D: Dialect>(
    lines: &[RawLine<'_>],
    dialect: &D,
    parse_findings: &mut Vec<ParseFinding>,
) -> Vec<Option<RegionKind>> {
    let mut regions = vec![None; lines.len()];
    let mut idx = 0usize;

    while idx < lines.len() {
        let opened = dialect
            .comment_region(lines[idx].raw)
            .map(|terminator| (RegionKind::Comment, terminator))
            .or_else(|| {
                dialect
                    .literal_region(lines[idx].raw)
                    .map(|terminator| (RegionKind::Literal, terminator))
            });
        let Some((kind, terminator)) = opened else {
            idx += 1;
            continue;
        };

        match (idx + 1..lines.len()).find(|&next| terminator.terminates(lines[next].raw)) {
            Some(end) => {
                regions[idx + 1..=end].fill(Some(kind));
                idx = end + 1;
            }
            None => {
                let (code, noun) = match kind {
                    RegionKind::Literal => ("unterminated-literal-region", "literal"),
                    RegionKind::Comment => ("unterminated-comment-region", "comment"),
                };
                parse_findings.push(ParseFinding {
                    code: code.to_string(),
                    message: format!(
                        "{noun} region is never closed by `{}`; its body is parsed as configuration",
                        terminator.pattern()
                    ),
                    span: lines[idx].span.clone(),
                });
                idx += 1;
            }
        }
    }

    regions
}

fn attach_node(doc: &mut Document, parent_stack: &[(usize, NodeId, bool)], id: NodeId) {
    if let Some((_, parent_id, _)) = parent_stack.last() {
        if !doc.add_child(*parent_id, id) {
            // if a parent cannot accept children for any reason, keep data by falling back to root.
            doc.roots.push(id);
        }
    } else {
        doc.roots.push(id);
    }
}

fn split_line_ending(segment: &str) -> (&str, &str) {
    if let Some(raw) = segment.strip_suffix("\r\n") {
        (raw, "\r\n")
    } else if let Some(raw) = segment.strip_suffix('\n') {
        (raw, "\n")
    } else {
        (segment, "")
    }
}

/// classify a line as blank, comment, or content based on the given comment
/// prefixes.
///
/// dialect-specific `classify_trivia` helpers delegate here so the
/// blank/comment/content logic lives in one place.
pub fn classify_trivia_with_prefixes(raw: &str, comment_prefixes: &[&str]) -> TriviaKind {
    if raw.trim().is_empty() {
        return TriviaKind::Blank;
    }

    let trimmed = raw.trim_start();
    if comment_prefixes.iter().any(|p| trimmed.starts_with(p)) {
        return TriviaKind::Comment;
    }

    TriviaKind::Content
}

fn classify_trivia(raw: &str) -> TriviaKind {
    classify_trivia_with_prefixes(raw, &["#", "!", "//"])
}

fn parse_parts(raw: &str) -> Option<ParsedLineParts> {
    let mut tokens = raw.split_whitespace();
    let head = tokens.next()?;
    let args = tokens.map(ToString::to_string).collect::<Vec<_>>();
    Some(ParsedLineParts {
        head: head.to_string(),
        args,
    })
}

/// split a content line into its `head` keyword and trailing `args`, using
/// [`tokenize`] with `punctuation`.
///
/// returns `None` when the line has no tokens.
pub fn parse_parts_with_punctuation(raw: &str, punctuation: &[char]) -> Option<ParsedLineParts> {
    let tokens = tokenize(raw, punctuation);
    let head = tokens.first()?.clone();
    let args = tokens.into_iter().skip(1).collect::<Vec<_>>();
    Some(ParsedLineParts { head, args })
}

/// tokenize a raw configuration line, splitting on whitespace while preserving
/// quoted strings. characters in `punctuation` are emitted as their own tokens
/// (flushing any accumulated word first), matching the Junos-style brace/semicolon
/// handling.
///
/// pass an empty slice for flat-line dialects (EOS, IOS XE) or `&['{', '}', ';']`
/// for hierarchical-brace dialects (Junos).
pub fn tokenize(raw: &str, punctuation: &[char]) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;
    let mut escape = false;

    for ch in raw.chars() {
        if let Some(q) = in_quote {
            if escape {
                current.push(ch);
                escape = false;
                continue;
            }

            if ch == '\\' {
                current.push(ch);
                escape = true;
                continue;
            }

            current.push(ch);
            if ch == q {
                in_quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => {
                current.push(ch);
                in_quote = Some(ch);
            }
            c if punctuation.contains(&c) => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    tokens.push(trimmed.to_string());
                }
                current.clear();
                tokens.push(ch.to_string());
            }
            c if c.is_whitespace() => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    tokens.push(trimmed.to_string());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        tokens.push(trimmed.to_string());
    }

    tokens
}

/// count the visual indentation width of a line, treating tabs as 4 spaces.
///
/// used by the parser to determine nesting depth; also useful for
/// normalization passes that need to re-indent mixed-whitespace lines.
pub fn count_indent(raw: &str) -> usize {
    let mut width = 0usize;
    for ch in raw.chars() {
        match ch {
            ' ' => width += 1,
            '\t' => width += 4,
            _ => break,
        }
    }
    width
}

fn has_mixed_leading_whitespace(raw: &str) -> bool {
    let mut seen_space = false;
    let mut seen_tab = false;
    for ch in raw.chars() {
        match ch {
            ' ' => seen_space = true,
            '\t' => seen_tab = true,
            _ => break,
        }
    }
    seen_space && seen_tab
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminator_contains_matches_anywhere_on_the_line() {
        let term = LiteralTerminator::Contains("^C".into());
        assert!(term.terminates("^C"));
        assert!(term.terminates("  ^C  "));
        assert!(term.terminates("goodbye^C"));
        assert!(!term.terminates("no delimiter here"));
        assert_eq!(term.pattern(), "^C");
    }

    #[test]
    fn terminator_exact_line_ignores_surrounding_whitespace_only() {
        let term = LiteralTerminator::ExactLine("EOF".into());
        assert!(term.terminates("EOF"));
        assert!(term.terminates("  EOF  "));
        assert!(!term.terminates("EOF and more"));
        assert!(!term.terminates("not EOF"));
        assert_eq!(term.pattern(), "EOF");
    }

    #[test]
    fn terminator_unescaped_quote_matches_a_quote_anywhere_on_the_line() {
        let term = LiteralTerminator::UnescapedQuote;
        assert!(term.terminates("\""));
        assert!(term.terminates("-----END CERTIFICATE-----\""));
        assert!(term.terminates("</html>\"  "));
        assert!(!term.terminates("MIIFDjBABgkqhkiG9w0BBQ0wMzAbBgkq"));
        assert!(!term.terminates(""));
        assert_eq!(term.pattern(), "\"");
    }

    #[test]
    fn terminator_unescaped_quote_respects_backslash_parity() {
        let term = LiteralTerminator::UnescapedQuote;
        assert!(!term.terminates(r#"<a href=\"help\">"#));
        assert!(term.terminates(r#"a windows path C:\\""#));
        assert!(!term.terminates(r#"C:\\\""#));
        assert!(term.terminates(r#"C:\\\\""#));
    }

    #[test]
    fn ends_inside_quoted_value_tracks_quote_state() {
        assert!(ends_inside_quoted_value(
            "        set private-key \"-----BEGIN ENCRYPTED PRIVATE KEY-----"
        ));
        assert!(!ends_inside_quoted_value("    set hostname \"FortiGate\""));
        assert!(!ends_inside_quoted_value("    edit \"port1\""));
        assert!(!ends_inside_quoted_value("        set range global"));
        assert!(!ends_inside_quoted_value(""));
    }

    #[test]
    fn ends_inside_quoted_value_ignores_escaped_quotes() {
        assert!(ends_inside_quoted_value(
            r#"        set buffer "<a href=\"/help\">Help</a>"#
        ));
        assert!(!ends_inside_quoted_value(
            r#"        set comment "he said \"hi\"""#
        ));
        assert!(ends_inside_quoted_value(r#"        set path "C:\\"#));
        assert!(!ends_inside_quoted_value(r#"        set path "C:\\""#));
    }

    #[test]
    fn ends_inside_quoted_value_handles_multi_byte_text() {
        assert!(ends_inside_quoted_value("        set comment \"Grüße"));
        assert!(!ends_inside_quoted_value("        set comment \"Grüße\""));
        assert!(!ends_inside_quoted_value(r#"        set comment "\é""#));
    }
}
