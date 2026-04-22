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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    pub key_hint: Option<String>,
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
    /// Optionally derive a stable identity hint for this line.
    fn key_hint(
        &self,
        _raw: &str,
        _parsed: Option<&ParsedLineParts>,
        _trivia: TriviaKind,
    ) -> Option<String> {
        None
    }
}

/// Parameterized dialect for IOS-like configuration text (EOS, IOS XE, NX-OS, …).
///
/// All IOS-like dialects share the same trivia classification, tokenization,
/// and key-hint derivation — the only thing that varies is the hint name stored
/// in [`DocumentMetadata`].  Construct with [`IosLikeDialect::new`]:
///
/// ```rust
/// use netform_ir::{IosLikeDialect, parse_with_dialect};
///
/// let doc = parse_with_dialect("hostname edge-1\n", &IosLikeDialect::new("iosxe"));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct IosLikeDialect {
    name: &'static str,
}

impl IosLikeDialect {
    /// Create a dialect instance tagged with the given hint name.
    pub const fn new(name: &'static str) -> Self {
        Self { name }
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
        ios_like_key_hint(parsed)
    }
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
    key_hint: Option<String>,
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
            key_hint: self.key_hint.clone(),
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
        let key_hint = dialect.key_hint(raw, parsed.as_ref(), trivia);

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
            key_hint,
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

/// Tokenize a raw configuration line, splitting on whitespace while preserving
/// quoted strings. Characters in `punctuation` are emitted as their own tokens
/// (flushing any accumulated word first), matching the Junos-style brace/semicolon
/// handling.
///
/// Pass an empty slice for flat-line dialects (EOS, IOS XE) or `&['{', '}', ';']`
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
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                }
                current.clear();
                tokens.push(ch.to_string());
            }
            c if c.is_whitespace() => {
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.trim().is_empty() {
        tokens.push(current.trim().to_string());
    }

    tokens
}

/// Derive a stable identity key for IOS-like configuration lines.
///
/// Shared between EOS and IOS XE dialects (both use the same flat-line
/// command grammar for structural headings like `interface`, `vlan`, etc.).
pub fn ios_like_key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    let parsed = parsed?;
    let head = parsed.head.as_str();
    let args = parsed.args.as_slice();

    match head {
        "interface" => args.first().map(|name| format!("interface:{name}")),
        "vlan" => args.first().map(|id| format!("vlan:{id}")),
        "vrf" => args.first().map(|name| format!("vrf:{name}")),
        "router" => match args {
            [proto, asn, ..] if proto == "bgp" => Some(format!("router:bgp:{asn}")),
            [proto, ..] => Some(format!("router:{proto}")),
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
        "ip" => match args {
            [next, kind, name, ..] if next == "access-list" => {
                Some(format!("ip-access-list:{kind}:{name}"))
            }
            [next, name, ..] if next == "prefix-list" => Some(format!("prefix-list:{name}")),
            [next, kind, name, ..] if next == "community-list" => {
                Some(format!("ip-community-list:{kind}:{name}"))
            }
            _ => None,
        },
        "access-list" => args.first().map(|num| format!("access-list:{num}")),
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
        // NX-OS-specific constructs
        "feature" => args.first().map(|name| format!("feature:{name}")),
        "vpc" => match args {
            [sub, id, ..] if sub == "domain" => Some(format!("vpc-domain:{id}")),
            _ => None,
        },
        "role" => match args {
            [sub, name, ..] if sub == "name" => Some(format!("role:{name}")),
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
        "system" => match args {
            [sub, ..] => Some(format!("system:{sub}")),
            _ => None,
        },
        _ => None,
    }
}

/// Classify trivia for IOS-like dialects (EOS, IOS XE).
///
/// Lines starting with `!` or `#` (after leading whitespace) are comments;
/// blank/whitespace-only lines are blank; everything else is content.
pub fn classify_ios_like_trivia(raw: &str) -> TriviaKind {
    if raw.trim().is_empty() {
        return TriviaKind::Blank;
    }

    let trimmed = raw.trim_start();
    if trimmed.starts_with('!') || trimmed.starts_with('#') {
        return TriviaKind::Comment;
    }

    TriviaKind::Content
}

/// Tokenize a content line for IOS-like dialects (EOS, IOS XE).
///
/// Uses [`tokenize`] with no punctuation characters, then splits the result
/// into a `head` keyword and trailing `args`.
pub fn parse_ios_like_parts(raw: &str) -> Option<ParsedLineParts> {
    let tokens = tokenize(raw, &[]);
    let head = tokens.first()?.clone();
    let args = tokens.into_iter().skip(1).collect::<Vec<_>>();
    Some(ParsedLineParts { head, args })
}

/// Count the visual indentation width of a line, treating tabs as 4 spaces.
///
/// Used by the parser to determine nesting depth; also useful for
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

    /// Helper: parse an IOS-like line and return the key hint.
    fn hint(line: &str) -> Option<String> {
        let parsed = parse_ios_like_parts(line);
        ios_like_key_hint(parsed.as_ref())
    }

    // -- existing constructs (regression) --

    #[test]
    fn key_hint_interface() {
        assert_eq!(
            hint("interface Ethernet1"),
            Some("interface:Ethernet1".into())
        );
    }

    #[test]
    fn key_hint_vlan() {
        assert_eq!(hint("vlan 100"), Some("vlan:100".into()));
    }

    #[test]
    fn key_hint_vrf() {
        assert_eq!(hint("vrf MGMT"), Some("vrf:MGMT".into()));
    }

    #[test]
    fn key_hint_router_bgp() {
        assert_eq!(hint("router bgp 65001"), Some("router:bgp:65001".into()));
    }

    #[test]
    fn key_hint_router_ospf() {
        assert_eq!(hint("router ospf"), Some("router:ospf".into()));
    }

    #[test]
    fn key_hint_route_map() {
        assert_eq!(
            hint("route-map REDISTRIBUTE permit 10"),
            Some("route-map:REDISTRIBUTE:permit:10".into()),
        );
    }

    #[test]
    fn key_hint_ip_access_list() {
        assert_eq!(
            hint("ip access-list extended BLOCK-RFC1918"),
            Some("ip-access-list:extended:BLOCK-RFC1918".into()),
        );
    }

    #[test]
    fn key_hint_ip_prefix_list() {
        assert_eq!(
            hint("ip prefix-list DEFAULT-ONLY"),
            Some("prefix-list:DEFAULT-ONLY".into()),
        );
    }

    #[test]
    fn key_hint_line() {
        assert_eq!(hint("line vty 0 4"), Some("line:vty:0:4".into()));
        assert_eq!(hint("line con 0"), Some("line:con:0".into()));
    }

    // -- new constructs --

    #[test]
    fn key_hint_class_map_match_all() {
        assert_eq!(
            hint("class-map match-all VOICE"),
            Some("class-map:VOICE".into()),
        );
    }

    #[test]
    fn key_hint_class_map_match_any() {
        assert_eq!(
            hint("class-map match-any WEB-TRAFFIC"),
            Some("class-map:WEB-TRAFFIC".into()),
        );
    }

    #[test]
    fn key_hint_class_map_bare() {
        assert_eq!(hint("class-map SIMPLE"), Some("class-map:SIMPLE".into()));
    }

    #[test]
    fn key_hint_policy_map() {
        assert_eq!(
            hint("policy-map QOS-POLICY"),
            Some("policy-map:QOS-POLICY".into()),
        );
    }

    #[test]
    fn key_hint_ip_community_list() {
        assert_eq!(
            hint("ip community-list standard COMM-LOCAL"),
            Some("ip-community-list:standard:COMM-LOCAL".into()),
        );
        assert_eq!(
            hint("ip community-list expanded COMM-TRANSIT"),
            Some("ip-community-list:expanded:COMM-TRANSIT".into()),
        );
    }

    #[test]
    fn key_hint_numbered_access_list() {
        assert_eq!(
            hint("access-list 100 permit ip any any"),
            Some("access-list:100".into()),
        );
        assert_eq!(
            hint("access-list 10 deny 10.0.0.0 0.255.255.255"),
            Some("access-list:10".into()),
        );
    }

    #[test]
    fn key_hint_crypto_map() {
        assert_eq!(
            hint("crypto map VPN-MAP 10 ipsec-isakmp"),
            Some("crypto:map:VPN-MAP".into()),
        );
    }

    #[test]
    fn key_hint_crypto_isakmp() {
        assert_eq!(
            hint("crypto isakmp policy 10"),
            Some("crypto:isakmp:policy".into()),
        );
    }

    #[test]
    fn key_hint_crypto_ikev2_proposal() {
        assert_eq!(
            hint("crypto ikev2 proposal PROP-1"),
            Some("crypto:ikev2:proposal:PROP-1".into()),
        );
    }

    #[test]
    fn key_hint_crypto_ikev2_policy() {
        assert_eq!(
            hint("crypto ikev2 policy POL-1"),
            Some("crypto:ikev2:policy:POL-1".into()),
        );
    }

    #[test]
    fn key_hint_crypto_ikev2_profile() {
        assert_eq!(
            hint("crypto ikev2 profile REMOTE-SITE"),
            Some("crypto:ikev2:profile:REMOTE-SITE".into()),
        );
    }

    #[test]
    fn key_hint_crypto_ipsec_transform_set() {
        assert_eq!(
            hint("crypto ipsec transform-set AES-SHA esp-aes esp-sha-hmac"),
            Some("crypto:ipsec:transform-set:AES-SHA".into()),
        );
    }

    #[test]
    fn key_hint_spanning_tree_vlan() {
        assert_eq!(
            hint("spanning-tree vlan 1-100 priority 4096"),
            Some("spanning-tree:vlan:1-100".into()),
        );
        assert_eq!(
            hint("spanning-tree vlan 200"),
            Some("spanning-tree:vlan:200".into()),
        );
    }

    #[test]
    fn key_hint_spanning_tree_no_match() {
        // Non-vlan spanning-tree commands should not produce a hint.
        assert_eq!(hint("spanning-tree mode rapid-pvst"), None);
    }

    // -- NX-OS-specific constructs --

    #[test]
    fn key_hint_feature() {
        assert_eq!(hint("feature ospf"), Some("feature:ospf".into()));
        assert_eq!(hint("feature bgp"), Some("feature:bgp".into()));
        assert_eq!(hint("feature vpc"), Some("feature:vpc".into()));
    }

    #[test]
    fn key_hint_vpc_domain() {
        assert_eq!(hint("vpc domain 10"), Some("vpc-domain:10".into()));
        assert_eq!(hint("vpc domain 100"), Some("vpc-domain:100".into()));
    }

    #[test]
    fn key_hint_vpc_no_domain() {
        // Bare vpc subcommands without "domain" should not produce a hint.
        assert_eq!(hint("vpc orphan-ports suspend"), None);
    }

    #[test]
    fn key_hint_role_name() {
        assert_eq!(
            hint("role name custom-admin"),
            Some("role:custom-admin".into()),
        );
        assert_eq!(
            hint("role name network-operator"),
            Some("role:network-operator".into()),
        );
    }

    #[test]
    fn key_hint_role_no_name() {
        assert_eq!(hint("role feature-group name"), None);
    }

    #[test]
    fn key_hint_monitor_session() {
        assert_eq!(hint("monitor session 1"), Some("monitor-session:1".into()),);
        assert_eq!(
            hint("monitor session 5 type erspan-source"),
            Some("monitor-session:5".into()),
        );
    }

    #[test]
    fn key_hint_monitor_no_session() {
        assert_eq!(hint("monitor copp-system-p-policy"), None);
    }

    #[test]
    fn key_hint_ntp_server() {
        assert_eq!(
            hint("ntp server 10.0.0.1"),
            Some("ntp:server:10.0.0.1".into()),
        );
        assert_eq!(
            hint("ntp server 2001:db8::1 prefer"),
            Some("ntp:server:2001:db8::1".into()),
        );
    }

    #[test]
    fn key_hint_ntp_peer() {
        assert_eq!(hint("ntp peer 10.0.0.2"), Some("ntp:peer:10.0.0.2".into()),);
    }

    #[test]
    fn key_hint_ntp_no_match() {
        // Other ntp subcommands should not produce a hint.
        assert_eq!(hint("ntp source-interface mgmt0"), None);
    }

    #[test]
    fn key_hint_system() {
        assert_eq!(hint("system jumbomtu 9216"), Some("system:jumbomtu".into()),);
        assert_eq!(
            hint("system nve infra-vlans 100"),
            Some("system:nve".into()),
        );
        assert_eq!(
            hint("system default switchport"),
            Some("system:default".into()),
        );
    }

    #[test]
    fn key_hint_none_for_unknown() {
        assert_eq!(hint("hostname ROUTER-1"), None);
    }

    #[test]
    fn key_hint_none_on_empty() {
        assert_eq!(ios_like_key_hint(None), None);
    }
}
