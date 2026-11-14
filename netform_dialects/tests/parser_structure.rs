use netform_ir::{
    Dialect, Document, Node, NodeId, ParsedLineParts, TriviaKind, parse_generic, parse_with_dialect,
};

type Parse = fn(&str) -> Document;

/// minimal dialect with `#` comments and a `}` block terminator.
struct BraceDialect;

impl Dialect for BraceDialect {
    fn classify_trivia(&self, raw: &str) -> TriviaKind {
        if raw.trim().is_empty() {
            TriviaKind::Blank
        } else if raw.trim_start().starts_with('#') {
            TriviaKind::Comment
        } else {
            TriviaKind::Content
        }
    }

    fn parse_parts(&self, _raw: &str) -> Option<ParsedLineParts> {
        None
    }

    fn block_terminator(&self, raw: &str) -> bool {
        raw.trim() == "}"
    }
}
/// each node's trimmed text, with a block's children in parentheses before its footer.
fn outline(doc: &Document, ids: &[NodeId]) -> String {
    ids.iter()
        .map(|id| match doc.node(*id).expect("node in arena") {
            Node::Line(line) => line.raw.trim().to_string(),
            Node::Block(block) => {
                let footer = block.footer.as_ref().map_or("", |footer| footer.raw.trim());
                let children = outline(doc, &block.children);
                format!("{} ({children}) {footer}", block.header.raw.trim())
                    .trim_end()
                    .to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn a_more_indented_comment_opens_the_block_above_it() {
    let brace = |input: &str| parse_with_dialect(input, &BraceDialect);
    let eos = netform_dialects::find("eos")
        .expect("eos is registered")
        .parse;
    let rows: &[(&str, Parse, &str, &str)] = &[
        (
            "comment-only body",
            parse_generic,
            "vrf instance A\n   !\nhostname x\n",
            "vrf instance A (!) | hostname x",
        ),
        (
            "comment under a leaf",
            parse_generic,
            "hostname x\n  ! note\n",
            "hostname x (! note)",
        ),
        (
            "column-0 separator",
            parse_generic,
            "interface Ethernet1\n!\ninterface Ethernet2\n   shutdown\n!\n",
            "interface Ethernet1 | ! | interface Ethernet2 (shutdown) | !",
        ),
        (
            "comment-only body before a terminator",
            brace,
            "system {\n    # note\n}\n",
            "system { (# note) }",
        ),
        (
            "same-indent terminator",
            brace,
            "system {\n# note\n}\n",
            "system { | # note | }",
        ),
        (
            "literal body",
            eos,
            "banner motd ^C\n  text\n^C\nhostname x\n",
            "banner motd ^C | text | ^C | hostname x",
        ),
    ];
    for &(case, parse, input, expected) in rows {
        let doc = parse(input);
        assert_eq!(doc.render(), input, "{case}");
        assert_eq!(outline(&doc, &doc.roots), expected, "{case}");
    }
}
