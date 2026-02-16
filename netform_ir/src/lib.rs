//! Lossless intermediate representation (IR) for network device configuration text.
//!
//! This crate provides:
//! - a tree model (`Document`, `Node`, `LineNode`, `BlockNode`)
//! - a conservative parser (`parse_generic`, `parse_with_dialect`)
//! - a lossless renderer (`Document::render`)
//!
//! The parser is intentionally conservative for pre-alpha use:
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

/// Stable arena identifier for a node in a [`Document`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub usize);

/// Location path used by diffs and diagnostics (`root_index`, then child indices).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Path(pub Vec<usize>);

/// Source span pointing to a single line and byte range in the original input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// Minimal tokenized representation of a content line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedLineParts {
    pub head: String,
    pub args: Vec<String>,
}

/// Lightweight classification used by parser, normalization, and diff views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriviaKind {
    Blank,
    Comment,
    Content,
    Unknown,
}

/// Leaf node preserving original raw text and parse metadata for one line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineNode {
    pub raw: String,
    pub line_ending: String,
    pub span: Span,
    pub parsed: Option<ParsedLineParts>,
    pub trivia: TriviaKind,
}

/// Structured block node with a header line and nested children.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockNode {
    pub header: LineNode,
    pub children: Vec<NodeId>,
    pub footer: Option<LineNode>,
    pub kind_label: Option<String>,
}

/// Arena node variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Node {
    Line(LineNode),
    Block(BlockNode),
}

/// Document metadata attached during parsing.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub source_name: Option<String>,
    pub dialect_hint: DialectHint,
    pub original_bytes: usize,
    pub line_count: usize,
    pub parse_findings: Vec<ParseFinding>,
}

/// Declared parser dialect used for this document.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DialectHint {
    #[default]
    Generic,
    Unknown,
    Named(String),
}

/// Parser-level uncertainty note attached to a source span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseFinding {
    pub code: String,
    pub message: String,
    pub span: Span,
}

/// Lossless parsed document backed by an arena and root node list.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Document {
    pub metadata: DocumentMetadata,
    pub roots: Vec<NodeId>,
    pub arena: Vec<Node>,
}

impl Document {
    /// Create an empty document with caller-supplied metadata.
    pub fn new(metadata: DocumentMetadata) -> Self {
        Self {
            metadata,
            roots: Vec::new(),
            arena: Vec::new(),
        }
    }

    /// Insert a node and register it as a root.
    pub fn insert_root(&mut self, node: Node) -> NodeId {
        let id = self.insert_node(node);
        self.roots.push(id);
        id
    }

    /// Insert a node into the arena and return its stable [`NodeId`].
    pub fn insert_node(&mut self, node: Node) -> NodeId {
        let id = NodeId(self.arena.len());
        self.arena.push(node);
        id
    }

    /// Borrow a node by id.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.arena.get(id.0)
    }

    /// Append `child` to `parent` if parent is a block.
    ///
    /// Returns `true` when attached, `false` when `parent` is not a block.
    pub fn add_child(&mut self, parent: NodeId, child: NodeId) -> bool {
        match self.arena.get_mut(parent.0) {
            Some(Node::Block(block)) => {
                block.children.push(child);
                true
            }
            _ => false,
        }
    }

    /// Render the document as exact original line bytes.
    ///
    /// For documents created with this crate's parser, this guarantees lossless
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

/// Parse input using the built-in generic dialect.
pub fn parse_generic(input: &str) -> Document {
    parse_with_dialect(input, &GenericDialect)
}

/// Dialect extension point for trivia classification and line tokenization.
pub trait Dialect {
    /// Report a dialect hint to store in [`DocumentMetadata`].
    fn dialect_hint(&self) -> DialectHint {
        DialectHint::Unknown
    }
    /// Classify a raw line into trivia/content buckets.
    fn classify_trivia(&self, raw: &str) -> TriviaKind;
    /// Optionally tokenize a raw content line into `head` + `args`.
    fn parse_parts(&self, raw: &str) -> Option<ParsedLineParts>;
}

/// Conservative default dialect for vendor-agnostic parsing.
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

/// Parse input into a lossless IR using the given dialect implementation.
///
/// Parsing is indentation-based and conservative:
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

    for idx in 0..lines.len() {
        let line = &lines[idx];

        if line.trivia == TriviaKind::Content && line.indent > 0 && parent_stack.is_empty() {
            doc.metadata.parse_findings.push(ParseFinding {
                code: "orphan-indentation".to_string(),
                message: "indented content line without an open parent block; line kept as-is"
                    .to_string(),
                span: line.span.clone(),
            });
        }

        // Non-blank lines can close open blocks when indentation decreases.
        if line.trivia != TriviaKind::Blank {
            while let Some((parent_indent, _)) = parent_stack.last().copied() {
                if line.indent <= parent_indent {
                    parent_stack.pop();
                } else {
                    break;
                }
            }
        }

        let opens_block = line.trivia == TriviaKind::Content
            && next_content_indent(&lines, idx).is_some_and(|next| next > line.indent);

        if opens_block {
            let block = Node::Block(BlockNode {
                header: line.as_line_node(),
                children: Vec::new(),
                footer: None,
                kind_label: None,
            });
            let id = doc.insert_node(block);
            attach_node(&mut doc, &parent_stack, id);
            parent_stack.push((line.indent, id));
        } else {
            let id = doc.insert_node(Node::Line(line.as_line_node()));
            attach_node(&mut doc, &parent_stack, id);
        }
    }

    doc
}

#[derive(Debug, Clone)]
struct LineCandidate {
    raw: String,
    line_ending: String,
    span: Span,
    parsed: Option<ParsedLineParts>,
    trivia: TriviaKind,
    indent: usize,
}

impl LineCandidate {
    fn as_line_node(&self) -> LineNode {
        LineNode {
            raw: self.raw.clone(),
            line_ending: self.line_ending.clone(),
            span: self.span.clone(),
            parsed: self.parsed.clone(),
            trivia: self.trivia,
        }
    }
}

fn collect_lines<D: Dialect>(
    input: &str,
    dialect: &D,
    line_count: &mut usize,
    parse_findings: &mut Vec<ParseFinding>,
) -> Vec<LineCandidate> {
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
        let trivia = dialect.classify_trivia(raw);
        let span = Span {
            line: line_no,
            start_byte: start,
            // Spans currently cover the content bytes only (not trailing newline bytes).
            end_byte: start + raw.len(),
        };
        let parsed = if trivia == TriviaKind::Content {
            dialect.parse_parts(raw)
        } else {
            None
        };

        if has_mixed_leading_whitespace(raw) {
            parse_findings.push(ParseFinding {
                code: "mixed-leading-whitespace".to_string(),
                message: "line indentation mixes spaces and tabs; structure may be ambiguous"
                    .to_string(),
                span: span.clone(),
            });
        }

        out.push(LineCandidate {
            raw: raw.to_string(),
            line_ending: line_ending.to_string(),
            span,
            parsed,
            trivia,
            indent: count_indent(raw),
        });

        *line_count += 1;
        line_no += 1;
        start = next_start;
    }

    out
}

fn next_content_indent(lines: &[LineCandidate], idx: usize) -> Option<usize> {
    lines[idx + 1..]
        .iter()
        .find(|line| line.trivia == TriviaKind::Content)
        .map(|line| line.indent)
}

fn attach_node(doc: &mut Document, parent_stack: &[(usize, NodeId)], id: NodeId) {
    if let Some((_, parent_id)) = parent_stack.last() {
        if !doc.add_child(*parent_id, id) {
            // If a parent cannot accept children for any reason, keep data by falling back to root.
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

fn classify_trivia(raw: &str) -> TriviaKind {
    if raw.trim().is_empty() {
        return TriviaKind::Blank;
    }

    let trimmed = raw.trim_start();
    if trimmed.starts_with('#') || trimmed.starts_with('!') || trimmed.starts_with("//") {
        return TriviaKind::Comment;
    }

    if raw.is_empty() {
        TriviaKind::Unknown
    } else {
        TriviaKind::Content
    }
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

fn count_indent(raw: &str) -> usize {
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
