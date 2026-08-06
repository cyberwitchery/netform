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

pub mod detect;

use serde::{Deserialize, Serialize};
use std::fmt;

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

    /// report whether a raw line opens a multi-line literal region, and what
    /// closes it.
    ///
    /// lines after the opener up to and including the terminator become
    /// [`TriviaKind::Literal`]; the opener itself stays [`TriviaKind::Content`].
    /// the returned pattern must be non-empty.
    fn literal_region(&self, _raw: &str) -> Option<LiteralTerminator> {
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
}

impl LiteralTerminator {
    /// report whether `raw` closes this region.
    pub fn terminates(&self, raw: &str) -> bool {
        match self {
            Self::Contains(pattern) => raw.contains(pattern.as_str()),
            Self::ExactLine(pattern) => raw.trim() == pattern,
        }
    }

    /// the text this terminator matches on, for diagnostics.
    pub fn pattern(&self) -> &str {
        match self {
            Self::Contains(pattern) | Self::ExactLine(pattern) => pattern,
        }
    }
}

/// parameterized dialect for IOS-like configuration text (EOS, IOS XE, NX-OS, …).
///
/// all IOS-like dialects share the same trivia classification and line
/// tokenization; they differ only in the hint name stored in
/// [`DocumentMetadata`] and the key-hint derivation function.  construct with
/// [`IosLikeDialect::new`], passing the dialect name and its key-hint function:
///
/// ```rust
/// use netform_ir::{IosLikeDialect, common_key_hint, parse_with_dialect};
///
/// let dialect = IosLikeDialect::new("iosxe", common_key_hint);
/// let doc = parse_with_dialect("hostname edge-1\n", &dialect);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct IosLikeDialect {
    name: &'static str,
    key_hint: fn(Option<&ParsedLineParts>) -> Option<String>,
}

impl IosLikeDialect {
    /// create a dialect instance tagged with the given hint name and key-hint
    /// derivation function.
    ///
    /// `key_hint` maps a parsed content line to its stable identity hint (or
    /// `None`); each IOS-family dialect crate passes its own so interface-type
    /// normalization and dialect-specific constructs stay local to that crate.
    pub const fn new(
        name: &'static str,
        key_hint: fn(Option<&ParsedLineParts>) -> Option<String>,
    ) -> Self {
        Self { name, key_hint }
    }
}

impl Dialect for IosLikeDialect {
    fn dialect_hint(&self) -> DialectHint {
        DialectHint::Named(self.name.to_string())
    }

    fn classify_trivia(&self, raw: &str) -> TriviaKind {
        classify_ios_like_trivia(raw)
    }

    fn parse_parts(&self, raw: &str) -> Option<ParsedLineParts> {
        parse_ios_like_parts(raw)
    }

    fn key_hint(
        &self,
        _raw: &str,
        parsed: Option<&ParsedLineParts>,
        trivia: TriviaKind,
    ) -> Option<String> {
        if trivia != TriviaKind::Content {
            return None;
        }
        (self.key_hint)(parsed)
    }

    fn literal_region(&self, raw: &str) -> Option<LiteralTerminator> {
        ios_like_literal_region(raw)
    }
}

/// recognize an IOS-family `banner <type> [delimiter]` line and report what
/// closes the free-text body that follows it.
///
/// - `banner motd ^C` — closed by the next line containing the delimiter.
/// - `banner motd #Warning` — a delimiter glued to the text, closed by the next `#`.
/// - `banner motd` — the delimiter-less EOS form, closed by a line reading `EOF`.
///
/// returns `None` for a banner whose delimiter reappears on the banner line
/// itself, which carries its whole body there.
///
/// # Example
///
/// ```rust
/// use netform_ir::{LiteralTerminator, ios_like_literal_region};
///
/// let region = ios_like_literal_region("banner motd ^C").unwrap();
/// assert!(region.terminates("^C"));
/// assert!(!region.terminates("Authorized use only"));
/// ```
pub fn ios_like_literal_region(raw: &str) -> Option<LiteralTerminator> {
    let after_head = raw.trim_start().strip_prefix("banner")?;
    if !after_head.starts_with([' ', '\t']) {
        return None;
    }

    let (_banner_type, after_type) = split_first_token(after_head)?;
    let Some((delimiter, tail)) = split_first_token(after_type) else {
        return Some(LiteralTerminator::ExactLine("EOF".to_string()));
    };

    if tail.contains(delimiter) {
        return None;
    }

    // a punctuation opener splits glued text off; an alphanumeric token is the
    // delimiter-less word form (`EOF`) and is never split.
    let opener = delimiter_opener(delimiter);
    if delimiter.len() > opener.len() {
        if delimiter.ends_with(opener) {
            return None;
        }
        if !opener.starts_with(char::is_alphanumeric) {
            if delimiter[opener.len()..].contains(opener) || tail.contains(opener) {
                return None;
            }
            return Some(LiteralTerminator::Contains(opener.to_string()));
        }
    }

    Some(LiteralTerminator::Contains(delimiter.to_string()))
}

/// the leading delimiter of a banner token: `^C` anywhere, any other `^X` only
/// as the whole token, otherwise the first character.
fn delimiter_opener(token: &str) -> &str {
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return token;
    };
    match chars.next() {
        Some(second)
            if first == '^'
                && (second == 'C' || token.len() == first.len_utf8() + second.len_utf8()) =>
        {
            &token[..first.len_utf8() + second.len_utf8()]
        }
        _ => &token[..first.len_utf8()],
    }
}

/// split leading whitespace and the first whitespace-delimited token off `raw`,
/// returning that token and the untrimmed remainder.
fn split_first_token(raw: &str) -> Option<(&str, &str)> {
    let trimmed = raw.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    Some(trimmed.split_at(end))
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
/// - open a block when next content line is more indented
/// - close blocks on non-blank dedent
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
    let mut parent_stack: Vec<(usize, NodeId)> = Vec::new();

    // pre-compute the indent of the next content line for each position in a
    // single reverse pass (O(n)), avoiding a per-line forward scan that would be
    // O(n²) on large configs.
    let mut next_content_indent: Vec<Option<usize>> = vec![None; lines.len()];
    {
        let mut last_content_indent: Option<usize> = None;
        for i in (0..lines.len()).rev() {
            next_content_indent[i] = last_content_indent;
            if lines[i].trivia == TriviaKind::Content {
                last_content_indent = Some(lines[i].indent);
            }
        }
    }

    // pre-compute block-opening decisions using the O(n) lookup table.
    let opens_block: Vec<bool> = (0..lines.len())
        .map(|idx| {
            lines[idx].trivia == TriviaKind::Content
                && next_content_indent[idx].is_some_and(|next| next > lines[idx].indent)
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

        // non-blank, non-literal lines can close open blocks when indentation decreases.
        let mut closed_block: Option<NodeId> = None;
        if !matches!(line.trivia, TriviaKind::Blank | TriviaKind::Literal) {
            while let Some((parent_indent, parent_id)) = parent_stack.last().copied() {
                if line.indent <= parent_indent {
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
            parent_stack.push((indent, id));
        } else {
            let id = doc.insert_node(Node::Line(line.into_line_node()));
            attach_node(&mut doc, &parent_stack, id);
        }
    }

    doc
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
}

impl LineCandidate {
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

    let literal = mark_literal_regions(&raw_lines, dialect, parse_findings);

    raw_lines
        .into_iter()
        .zip(literal)
        .map(|(line, is_literal)| {
            let RawLine {
                raw,
                line_ending,
                span,
            } = line;

            let trivia = if is_literal {
                TriviaKind::Literal
            } else {
                dialect.classify_trivia(raw)
            };
            let parsed = if trivia == TriviaKind::Content {
                dialect.parse_parts(raw)
            } else {
                None
            };
            let key_hint = dialect.key_hint(raw, parsed.as_ref(), trivia);

            if !is_literal && has_mixed_leading_whitespace(raw) {
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

/// mark every line that falls inside a dialect literal region.
///
/// an opener whose terminator never appears is not entered; it gets an
/// `unterminated-literal-region` finding instead.
fn mark_literal_regions<D: Dialect>(
    lines: &[RawLine<'_>],
    dialect: &D,
    parse_findings: &mut Vec<ParseFinding>,
) -> Vec<bool> {
    let mut literal = vec![false; lines.len()];
    let mut idx = 0usize;

    while idx < lines.len() {
        let Some(terminator) = dialect.literal_region(lines[idx].raw) else {
            idx += 1;
            continue;
        };

        match (idx + 1..lines.len()).find(|&next| terminator.terminates(lines[next].raw)) {
            Some(end) => {
                literal[idx + 1..=end].fill(true);
                idx = end + 1;
            }
            None => {
                parse_findings.push(ParseFinding {
                    code: "unterminated-literal-region".to_string(),
                    message: format!(
                        "literal region is never closed by `{}`; its body is parsed as configuration",
                        terminator.pattern()
                    ),
                    span: lines[idx].span.clone(),
                });
                idx += 1;
            }
        }
    }

    literal
}

fn attach_node(doc: &mut Document, parent_stack: &[(usize, NodeId)], id: NodeId) {
    if let Some((_, parent_id)) = parent_stack.last() {
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

/// configuration for [`ios_family_key_hint`], parameterizing the constructs
/// that differ across IOS-family dialects while sharing the common structure.
pub struct IosKeyHintConfig {
    /// interface type prefixes for normalization (longest-prefix-first).
    pub interface_types: &'static [&'static str],
    /// the VRF sub-command keyword (`"instance"`, `"definition"`, `"context"`).
    pub vrf_keyword: &'static str,
    /// router protocols (beyond BGP and OSPF) whose second argument should be
    /// included in the hint — e.g. `&["eigrp"]` for IOS XE.
    pub extra_router_protos: &'static [&'static str],
}

/// derive a key hint for constructs shared across IOS-family dialects.
///
/// handles `interface`, `vrf`, `router`, and `ip` using the provided
/// [`IosKeyHintConfig`].  returns `None` when the head keyword is not one of
/// these four, allowing the caller to try dialect-specific arms before falling
/// back to [`common_key_hint`].
pub fn ios_family_key_hint(
    parsed: Option<&ParsedLineParts>,
    config: &IosKeyHintConfig,
) -> Option<String> {
    let parsed_ref = parsed?;
    let head = parsed_ref.head.as_str();
    let args = parsed_ref.args.as_slice();

    match head {
        "interface" => {
            let name = args.first()?;
            if let Some((itype, id)) = parse_interface(name, config.interface_types) {
                Some(format!("interface:{itype}:{id}"))
            } else {
                Some(format!("interface:{name}"))
            }
        }
        "vrf" => match args {
            [sub, name, ..] if sub == config.vrf_keyword => Some(format!("vrf:{name}")),
            [name, ..] => Some(format!("vrf:{name}")),
            _ => None,
        },
        "router" => match args {
            [proto, id, ..] if proto == "bgp" => Some(format!("router:bgp:{id}")),
            [proto, id, ..] if proto == "ospf" => Some(format!("router:ospf:{id}")),
            [proto, id, ..] if config.extra_router_protos.contains(&proto.as_str()) => {
                Some(format!("router:{proto}:{id}"))
            }
            [proto, ..] => Some(format!("router:{proto}")),
            _ => None,
        },
        "ip" => match args {
            [next, kind, name, ..] if next == "access-list" => {
                Some(format!("ip-access-list:{kind}:{name}"))
            }
            [next, name] if next == "access-list" => Some(format!("ip-access-list:{name}")),
            [next, name, ..] if next == "prefix-list" => Some(format!("prefix-list:{name}")),
            [next, kind, name, ..] if next == "community-list" => {
                Some(format!("ip-community-list:{kind}:{name}"))
            }
            [next, vrf_kw, vrf_name, prefix, ..] if next == "route" && vrf_kw == "vrf" => {
                Some(format!("ip-route:{vrf_name}:{prefix}"))
            }
            [next, prefix, ..] if next == "route" => Some(format!("ip-route:{prefix}")),
            _ => None,
        },
        _ => None,
    }
}

/// derive a stable identity key for constructs shared across all IOS-like
/// dialects (EOS, IOS XE, NX-OS).
///
/// this covers the match arms that are identical in every IOS-family dialect:
/// `vlan`, `route-map`, `class-map`, `policy-map`, `ipv6`, `crypto`,
/// `spanning-tree`, `line`, `monitor`, and `ntp`.  dialect-specific
/// functions should match their own constructs first, then fall back here.
///
/// numbered `access-list N ...` rules are intentionally *not* keyed here:
/// they are ordered sequence entries whose identity is their full text, so
/// they must key on the line text (via no hint) rather than the shared ACL
/// number — otherwise a rule-body change is invisible to the diff.
pub fn common_key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    let parsed = parsed?;
    let head = parsed.head.as_str();
    let args = parsed.args.as_slice();

    match head {
        "vlan" => match args {
            // NX-OS `vlan configuration <id>` is a distinct per-VLAN block;
            // key it on the id so different ids don't collapse to the literal
            // "configuration" (and collide with each other).
            [sub, id, ..] if sub == "configuration" => Some(format!("vlan-configuration:{id}")),
            [id, ..] => Some(format!("vlan:{id}")),
            _ => None,
        },
        "route-map" => match args {
            [name, action, seq, ..] => Some(format!("route-map:{name}:{action}:{seq}")),
            [name, action] => Some(format!("route-map:{name}:{action}")),
            _ => None,
        },
        "class-map" => match args {
            [_match_kind, name, ..] => Some(format!("class-map:{name}")),
            [name] => Some(format!("class-map:{name}")),
            _ => None,
        },
        "policy-map" => args.first().map(|name| format!("policy-map:{name}")),
        "ipv6" => match args {
            [next, name, ..] if next == "access-list" => Some(format!("ipv6-access-list:{name}")),
            [next, name, ..] if next == "prefix-list" => Some(format!("ipv6-prefix-list:{name}")),
            [next, vrf_kw, vrf_name, prefix, ..] if next == "route" && vrf_kw == "vrf" => {
                Some(format!("ipv6-route:{vrf_name}:{prefix}"))
            }
            [next, prefix, ..] if next == "route" => Some(format!("ipv6-route:{prefix}")),
            _ => None,
        },
        "crypto" => match args {
            [kind, sub, name, ..] if kind == "ikev2" => Some(format!("crypto:ikev2:{sub}:{name}")),
            [kind, sub, name, ..] if kind == "ipsec" => Some(format!("crypto:ipsec:{sub}:{name}")),
            [kind, name, ..] if kind == "map" => Some(format!("crypto:map:{name}")),
            [kind, num, ..] if kind == "isakmp" => Some(format!("crypto:isakmp:{num}")),
            _ => None,
        },
        "spanning-tree" => match args {
            [next, id, ..] if next == "vlan" => Some(format!("spanning-tree:vlan:{id}")),
            _ => None,
        },
        "line" => match args {
            [kind, from, to, ..] => Some(format!("line:{kind}:{from}:{to}")),
            [kind, one, ..] => Some(format!("line:{kind}:{one}")),
            _ => None,
        },
        "monitor" => match args {
            [sub, id, ..] if sub == "session" => Some(format!("monitor-session:{id}")),
            _ => None,
        },
        "ntp" => match args {
            [kind, addr, ..] if kind == "server" || kind == "peer" => {
                Some(format!("ntp:{kind}:{addr}"))
            }
            _ => None,
        },
        _ => None,
    }
}

/// parse an interface name into `(canonical_type, id)` using the given type
/// prefix table.
///
/// uses case-insensitive prefix matching so that any casing of a known
/// interface type normalizes to the canonical lowercase form.  the `types`
/// slice must be ordered longest-prefix-first so that e.g. `tengigabitethernet`
/// matches before `gigabitethernet`.
///
/// returns `None` if the name doesn't match any entry in `types` or has no ID
/// portion after the prefix.
///
/// # Example
///
/// ```rust
/// use netform_ir::parse_interface;
///
/// const TYPES: &[&str] = &["ethernet", "loopback"];
/// assert_eq!(parse_interface("Ethernet1", TYPES), Some(("ethernet", "1")));
/// assert_eq!(parse_interface("Loopback0", TYPES), Some(("loopback", "0")));
/// assert_eq!(parse_interface("Serial0/0", TYPES), None);
/// ```
pub fn parse_interface<'a>(
    name: &'a str,
    types: &[&'static str],
) -> Option<(&'static str, &'a str)> {
    let lower = name.to_ascii_lowercase();
    for &canonical in types {
        if lower.starts_with(canonical) && name.len() > canonical.len() {
            let id = &name[canonical.len()..];
            return Some((canonical, id));
        }
    }
    None
}

/// classify trivia for IOS-like dialects (EOS, IOS XE).
///
/// lines starting with `!` or `#` (after leading whitespace) are comments;
/// blank/whitespace-only lines are blank; everything else is content.
pub fn classify_ios_like_trivia(raw: &str) -> TriviaKind {
    classify_trivia_with_prefixes(raw, &["!", "#"])
}

/// tokenize a content line for IOS-like dialects (EOS, IOS XE).
///
/// uses [`tokenize`] with no punctuation characters, then splits the result
/// into a `head` keyword and trailing `args`.
pub fn parse_ios_like_parts(raw: &str) -> Option<ParsedLineParts> {
    let tokens = tokenize(raw, &[]);
    let head = tokens.first()?.clone();
    let args = tokens.into_iter().skip(1).collect::<Vec<_>>();
    Some(ParsedLineParts { head, args })
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

    fn common_hint(line: &str) -> Option<String> {
        let parsed = parse_ios_like_parts(line);
        common_key_hint(parsed.as_ref())
    }

    #[test]
    fn common_key_hint_vlan() {
        assert_eq!(common_hint("vlan 100"), Some("vlan:100".into()));
    }

    #[test]
    fn common_key_hint_vlan_configuration() {
        assert_eq!(
            common_hint("vlan configuration 10"),
            Some("vlan-configuration:10".into()),
        );
        assert_eq!(common_hint("vlan 100"), Some("vlan:100".into()));
    }

    #[test]
    fn common_key_hint_route_map() {
        assert_eq!(
            common_hint("route-map REDISTRIBUTE permit 10"),
            Some("route-map:REDISTRIBUTE:permit:10".into()),
        );
    }

    #[test]
    fn common_key_hint_class_map() {
        assert_eq!(
            common_hint("class-map match-all VOICE"),
            Some("class-map:VOICE".into()),
        );
    }

    #[test]
    fn common_key_hint_policy_map() {
        assert_eq!(
            common_hint("policy-map QOS-POLICY"),
            Some("policy-map:QOS-POLICY".into()),
        );
    }

    #[test]
    fn common_key_hint_access_list_has_no_hint() {
        // see key_hint_numbered_access_list_has_no_hint.
        assert_eq!(common_hint("access-list 100 permit ip any any"), None);
    }

    #[test]
    fn common_key_hint_crypto() {
        assert_eq!(
            common_hint("crypto ikev2 proposal PROP-1"),
            Some("crypto:ikev2:proposal:PROP-1".into()),
        );
    }

    #[test]
    fn common_key_hint_spanning_tree() {
        assert_eq!(
            common_hint("spanning-tree vlan 1-100 priority 4096"),
            Some("spanning-tree:vlan:1-100".into()),
        );
    }

    #[test]
    fn common_key_hint_line() {
        assert_eq!(common_hint("line vty 0 4"), Some("line:vty:0:4".into()));
    }

    #[test]
    fn common_key_hint_monitor() {
        assert_eq!(
            common_hint("monitor session 1"),
            Some("monitor-session:1".into()),
        );
    }

    #[test]
    fn common_key_hint_ntp() {
        assert_eq!(
            common_hint("ntp server 10.0.0.1"),
            Some("ntp:server:10.0.0.1".into()),
        );
    }

    #[test]
    fn common_key_hint_ipv6() {
        assert_eq!(
            common_hint("ipv6 access-list BLOCK-BOGONS"),
            Some("ipv6-access-list:BLOCK-BOGONS".into()),
        );
    }

    #[test]
    fn common_key_hint_none_for_dialect_specific() {
        // constructs handled by dialect-specific functions should return None.
        assert_eq!(common_hint("interface Ethernet1"), None);
        assert_eq!(common_hint("vrf MGMT"), None);
        assert_eq!(common_hint("router bgp 65001"), None);
        assert_eq!(common_hint("ip access-list extended ACL"), None);
    }

    #[test]
    fn common_key_hint_none_on_empty() {
        assert_eq!(common_key_hint(None), None);
    }

    #[test]
    fn common_key_hint_class_map_match_any() {
        assert_eq!(
            common_hint("class-map match-any WEB-TRAFFIC"),
            Some("class-map:WEB-TRAFFIC".into()),
        );
    }

    #[test]
    fn common_key_hint_class_map_bare() {
        assert_eq!(
            common_hint("class-map SIMPLE"),
            Some("class-map:SIMPLE".into())
        );
    }

    #[test]
    fn common_key_hint_crypto_map() {
        assert_eq!(
            common_hint("crypto map VPN-MAP 10 ipsec-isakmp"),
            Some("crypto:map:VPN-MAP".into()),
        );
    }

    #[test]
    fn common_key_hint_crypto_isakmp() {
        assert_eq!(
            common_hint("crypto isakmp policy 10"),
            Some("crypto:isakmp:policy".into()),
        );
    }

    #[test]
    fn common_key_hint_crypto_ipsec() {
        assert_eq!(
            common_hint("crypto ipsec transform-set AES-SHA esp-aes esp-sha-hmac"),
            Some("crypto:ipsec:transform-set:AES-SHA".into()),
        );
    }

    #[test]
    fn common_key_hint_ipv6_prefix_list() {
        assert_eq!(
            common_hint("ipv6 prefix-list DEFAULT-V6-ONLY"),
            Some("ipv6-prefix-list:DEFAULT-V6-ONLY".into()),
        );
    }

    #[test]
    fn common_key_hint_ipv6_route() {
        assert_eq!(
            common_hint("ipv6 route 2001:db8::/32 Null0"),
            Some("ipv6-route:2001:db8::/32".into()),
        );
    }

    #[test]
    fn common_key_hint_ipv6_route_vrf() {
        assert_eq!(
            common_hint("ipv6 route vrf MGMT ::/0 GigabitEthernet0/0 fe80::1"),
            Some("ipv6-route:MGMT:::/0".into()),
        );
    }

    #[test]
    fn common_key_hint_ipv6_no_match() {
        assert_eq!(common_hint("ipv6 unicast-routing"), None);
    }

    #[test]
    fn common_key_hint_spanning_tree_no_match() {
        assert_eq!(common_hint("spanning-tree mode rapid-pvst"), None);
    }

    #[test]
    fn common_key_hint_monitor_no_session() {
        assert_eq!(common_hint("monitor copp-system-p-policy"), None);
    }

    #[test]
    fn common_key_hint_ntp_peer() {
        assert_eq!(
            common_hint("ntp peer 10.0.0.2"),
            Some("ntp:peer:10.0.0.2".into())
        );
    }

    #[test]
    fn common_key_hint_ntp_no_match() {
        assert_eq!(common_hint("ntp source-interface mgmt0"), None);
    }

    #[test]
    fn literal_region_delimiter_form() {
        assert_eq!(
            ios_like_literal_region("banner motd ^C"),
            Some(LiteralTerminator::Contains("^C".into())),
        );
        assert_eq!(
            ios_like_literal_region("banner login #"),
            Some(LiteralTerminator::Contains("#".into())),
        );
    }

    #[test]
    fn literal_region_accepts_control_byte_delimiter() {
        assert_eq!(
            ios_like_literal_region("banner motd \u{3}"),
            Some(LiteralTerminator::Contains("\u{3}".into())),
        );
    }

    #[test]
    fn literal_region_delimiter_less_eos_form() {
        assert_eq!(
            ios_like_literal_region("banner motd"),
            Some(LiteralTerminator::ExactLine("EOF".into())),
        );
    }

    #[test]
    fn literal_region_declines_self_contained_banner() {
        assert_eq!(ios_like_literal_region("banner motd ^C Hi there ^C"), None);
        assert_eq!(ios_like_literal_region("banner motd ^CHi^C"), None);
        assert_eq!(ios_like_literal_region("banner motd ^C^C"), None);
        assert_eq!(ios_like_literal_region("banner motd #Hi#"), None);
    }

    #[test]
    fn literal_region_keeps_multi_character_delimiters_whole() {
        assert_eq!(
            ios_like_literal_region("banner motd EOF"),
            Some(LiteralTerminator::Contains("EOF".into())),
        );
        assert_eq!(ios_like_literal_region("banner motd \u{3}Hi\u{3}"), None,);
    }

    #[test]
    fn literal_region_splits_caret_delimiter_from_glued_text() {
        assert_eq!(
            ios_like_literal_region("banner motd ^CHi"),
            Some(LiteralTerminator::Contains("^C".into())),
        );
    }

    #[test]
    fn literal_region_splits_a_plain_caret_delimiter_from_glued_text() {
        assert_eq!(
            ios_like_literal_region("banner motd ^Warning restricted"),
            Some(LiteralTerminator::Contains("^".into())),
        );
        assert_eq!(
            ios_like_literal_region("banner motd ^1st line"),
            Some(LiteralTerminator::Contains("^".into())),
        );
    }

    #[test]
    fn literal_region_keeps_a_standalone_caret_escape_whole() {
        for delimiter in ["^C", "^Z", "^A", "^X", "^^", "^é"] {
            assert_eq!(
                ios_like_literal_region(&format!("banner motd {delimiter}")),
                Some(LiteralTerminator::Contains(delimiter.into())),
                "{delimiter}",
            );
            assert_eq!(
                ios_like_literal_region(&format!("banner motd {delimiter} Authorized use only")),
                Some(LiteralTerminator::Contains(delimiter.into())),
                "{delimiter}",
            );
        }
    }

    #[test]
    fn literal_region_splits_a_non_c_caret_escape_glued_to_its_text() {
        // `^Z` + `danger` and `^` + `Zdanger` are indistinguishable (see `delimiter_opener`).
        assert_eq!(
            ios_like_literal_region("banner motd ^Zdanger"),
            Some(LiteralTerminator::Contains("^".into())),
        );
    }

    #[test]
    fn literal_region_splits_punctuation_delimiter_from_glued_text() {
        assert_eq!(
            ios_like_literal_region("banner motd #Warning restricted"),
            Some(LiteralTerminator::Contains("#".into())),
        );
        assert_eq!(
            ios_like_literal_region("banner login @Hi"),
            Some(LiteralTerminator::Contains("@".into())),
        );
        assert_eq!(
            ios_like_literal_region("banner motd \u{3}Warning"),
            Some(LiteralTerminator::Contains("\u{3}".into())),
        );
    }

    #[test]
    fn literal_region_declines_a_one_line_banner_whose_text_contains_a_space() {
        assert_eq!(ios_like_literal_region("banner motd #Hello world#"), None);
        assert_eq!(ios_like_literal_region("banner motd ^CHello world^C"), None);
        assert_eq!(ios_like_literal_region("banner motd ^Hello world^"), None);
        assert_eq!(
            ios_like_literal_region("banner exec %Please log out at %the end%"),
            None,
        );
        assert_eq!(
            ios_like_literal_region("banner motd \u{3}Hello world\u{3}"),
            None,
        );
        assert_eq!(
            ios_like_literal_region("banner motd #Hello world"),
            Some(LiteralTerminator::Contains("#".into())),
        );
        assert_eq!(
            ios_like_literal_region("banner motd ^CHello world"),
            Some(LiteralTerminator::Contains("^C".into())),
        );
        assert_eq!(
            ios_like_literal_region("banner motd ^Hello world"),
            Some(LiteralTerminator::Contains("^".into())),
        );
    }

    #[test]
    fn literal_region_opens_when_text_starts_on_the_banner_line() {
        assert_eq!(
            ios_like_literal_region("banner motd ^C Authorized use only"),
            Some(LiteralTerminator::Contains("^C".into())),
        );
    }

    #[test]
    fn literal_region_declines_non_banner_lines() {
        assert_eq!(ios_like_literal_region("interface Ethernet1"), None);
        assert_eq!(ios_like_literal_region("bannermotd ^C"), None);
        assert_eq!(ios_like_literal_region("no banner motd"), None);
        assert_eq!(ios_like_literal_region("banner"), None);
        assert_eq!(ios_like_literal_region("banner   "), None);
    }

    #[test]
    fn literal_region_ignores_leading_indentation() {
        assert_eq!(
            ios_like_literal_region("   banner motd ^C"),
            Some(LiteralTerminator::Contains("^C".into())),
        );
    }

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
}
