//! parser coverage for multi-line literal regions.

use netform_ir::{
    Dialect, LiteralTerminator, Node, ParsedLineParts, TriviaKind, parse_generic,
    parse_with_dialect,
};

/// dialect whose only customization is a literal region, independent of any
/// vendor banner syntax.
struct MarkerDialect;

impl Dialect for MarkerDialect {
    fn classify_trivia(&self, raw: &str) -> TriviaKind {
        if raw.trim().is_empty() {
            TriviaKind::Blank
        } else {
            TriviaKind::Content
        }
    }

    fn parse_parts(&self, _raw: &str) -> Option<ParsedLineParts> {
        None
    }

    fn literal_region(&self, raw: &str) -> Option<LiteralTerminator> {
        (raw.trim() == "BEGIN").then(|| LiteralTerminator::ExactLine("END".to_string()))
    }
}

/// dialect returning [`TriviaKind::Literal`] from `classify_trivia`, with no region.
struct LiteralTriviaDialect;

impl Dialect for LiteralTriviaDialect {
    fn classify_trivia(&self, raw: &str) -> TriviaKind {
        if raw.trim().is_empty() {
            TriviaKind::Blank
        } else if raw.trim_start().starts_with("LIT ") {
            TriviaKind::Literal
        } else {
            TriviaKind::Content
        }
    }

    fn parse_parts(&self, _raw: &str) -> Option<ParsedLineParts> {
        None
    }
}

fn line_kinds(doc: &netform_ir::Document) -> Vec<(String, TriviaKind)> {
    fn walk(
        doc: &netform_ir::Document,
        id: netform_ir::NodeId,
        out: &mut Vec<(String, TriviaKind)>,
    ) {
        match doc.node(id).expect("node in arena") {
            Node::Line(line) => out.push((line.raw.clone(), line.trivia)),
            Node::Block(block) => {
                out.push((block.header.raw.clone(), block.header.trivia));
                for child in &block.children {
                    walk(doc, *child, out);
                }
                if let Some(footer) = &block.footer {
                    out.push((footer.raw.clone(), footer.trivia));
                }
            }
        }
    }

    let mut out = Vec::new();
    for root in &doc.roots {
        walk(doc, *root, &mut out);
    }
    out
}

fn trivia_of(doc: &netform_ir::Document, raw: &str) -> TriviaKind {
    line_kinds(doc)
        .into_iter()
        .find(|(text, _)| text == raw)
        .unwrap_or_else(|| panic!("line {raw:?} not found in document"))
        .1
}

#[test]
fn a_dialect_classified_literal_line_does_not_close_an_enclosing_block() {
    let input = "outer\n  inner\n    child\nLIT free text\n    sibling\n";
    let doc = parse_with_dialect(input, &LiteralTriviaDialect);

    assert_eq!(doc.render(), input);
    assert_eq!(doc.roots.len(), 1);

    let Some(Node::Block(outer)) = doc.node(doc.roots[0]) else {
        panic!("expected an `outer` block");
    };
    let Some(Node::Block(inner)) = doc.node(outer.children[0]) else {
        panic!("expected an `inner` block");
    };
    assert_eq!(
        inner
            .children
            .iter()
            .map(|id| match doc.node(*id) {
                Some(Node::Line(line)) => line.raw.as_str(),
                _ => panic!("expected a leaf line"),
            })
            .collect::<Vec<_>>(),
        vec!["    child", "LIT free text", "    sibling"],
    );
}

#[test]
fn generic_dialect_has_no_literal_regions() {
    let doc = parse_generic("banner motd ^C\n! still a comment\n^C\n");

    assert_eq!(trivia_of(&doc, "! still a comment"), TriviaKind::Comment);
    assert!(doc.metadata.parse_findings.is_empty());
}

#[test]
fn custom_dialect_region_is_honored() {
    let input = "alpha\nBEGIN\nanything at all\n  indented\nEND\nbeta\n";
    let doc = parse_with_dialect(input, &MarkerDialect);

    assert_eq!(
        line_kinds(&doc),
        vec![
            ("alpha".to_string(), TriviaKind::Content),
            ("BEGIN".to_string(), TriviaKind::Content),
            ("anything at all".to_string(), TriviaKind::Literal),
            ("  indented".to_string(), TriviaKind::Literal),
            ("END".to_string(), TriviaKind::Literal),
            ("beta".to_string(), TriviaKind::Content),
        ],
    );
}
