use netform_ir::{Dialect, Node, ParsedLineParts, TriviaKind, parse_generic, parse_with_dialect};

/// minimal dialect whose only customization is a `}` block terminator, used to
/// exercise footer attachment independently of any vendor dialect crate.
struct BraceDialect;

impl Dialect for BraceDialect {
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

    fn block_terminator(&self, raw: &str) -> bool {
        raw.trim() == "}"
    }
}

#[test]
fn builds_blocks_from_indentation() {
    let input = "interface Ethernet1/1\n  description Uplink\n  ip address 10.0.0.1/31\nrouter bgp 65000\n  neighbor 10.0.0.0 remote-as 65001\n! trailing\n";

    let doc = parse_generic(input);
    assert_eq!(doc.roots.len(), 3);

    match doc.node(doc.roots[0]).expect("root 0") {
        Node::Block(block) => {
            assert_eq!(block.header.raw, "interface Ethernet1/1");
            assert_eq!(block.children.len(), 2);
        }
        _ => panic!("expected first root to be a block"),
    }

    match doc.node(doc.roots[1]).expect("root 1") {
        Node::Block(block) => {
            assert_eq!(block.header.raw, "router bgp 65000");
            assert_eq!(block.children.len(), 1);
        }
        _ => panic!("expected second root to be a block"),
    }

    match doc.node(doc.roots[2]).expect("root 2") {
        Node::Line(line) => {
            assert_eq!(line.trivia, TriviaKind::Comment);
            assert_eq!(line.raw, "! trailing");
        }
        _ => panic!("expected third root to be a line"),
    }
}

#[test]
fn keeps_flat_structure_when_no_indent_signal() {
    let input =
        "set system host-name edge-01\nset system services ssh\nset system login user admin\n";

    let doc = parse_generic(input);
    assert_eq!(doc.roots.len(), 3);
    assert!(
        doc.roots
            .iter()
            .all(|id| matches!(doc.node(*id), Some(Node::Line(_))))
    );
}

#[test]
fn records_finding_for_mixed_leading_whitespace() {
    let input = "interface Ethernet1\n \t description mixed\n";
    let doc = parse_generic(input);

    assert!(
        doc.metadata
            .parse_findings
            .iter()
            .any(|f| f.code == "mixed-leading-whitespace")
    );
}

#[test]
fn records_finding_for_orphan_indentation() {
    let input = "  orphan-child-line\n";
    let doc = parse_generic(input);

    assert!(
        doc.metadata
            .parse_findings
            .iter()
            .any(|f| f.code == "orphan-indentation")
    );
    assert_eq!(doc.render(), input);
}

#[test]
fn block_terminator_is_attached_as_footer() {
    let input = "root {\n    child a\n    child b\n}\n";
    let doc = parse_with_dialect(input, &BraceDialect);

    assert_eq!(doc.render(), input, "footer attachment must round-trip");
    assert_eq!(doc.roots.len(), 1, "terminator is not a separate root");

    match doc.node(doc.roots[0]).expect("root 0") {
        Node::Block(block) => {
            assert_eq!(block.header.raw, "root {");
            assert_eq!(block.children.len(), 2);
            assert_eq!(
                block.footer.as_ref().map(|f| f.raw.as_str()),
                Some("}"),
                "closing brace should be the block footer",
            );
        }
        _ => panic!("expected the root to be a block"),
    }
}

#[test]
fn nested_terminators_close_their_own_block() {
    let input = "outer {\n    inner {\n        leaf\n    }\n}\n";
    let doc = parse_with_dialect(input, &BraceDialect);

    assert_eq!(doc.render(), input);
    let Node::Block(outer) = doc.node(doc.roots[0]).expect("outer") else {
        panic!("expected outer block");
    };
    assert_eq!(outer.footer.as_ref().map(|f| f.raw.as_str()), Some("}"));
    assert_eq!(outer.children.len(), 1);

    let Node::Block(inner) = doc.node(outer.children[0]).expect("inner") else {
        panic!("expected inner block");
    };
    assert_eq!(
        inner.footer.as_ref().map(|f| f.raw.as_str()),
        Some("    }"),
        "the indented brace closes the inner block, not the outer one",
    );
}

#[test]
fn generic_dialect_does_not_attach_footers() {
    // the generic dialect has no block terminators, so a `}` line dedents to a
    // plain sibling and the block keeps `footer: None`.
    let input = "root {\n    child a\n}\n";
    let doc = parse_generic(input);

    match doc.node(doc.roots[0]).expect("root 0") {
        Node::Block(block) => assert!(
            block.footer.is_none(),
            "generic dialect must not produce footers",
        ),
        _ => panic!("expected the root to be a block"),
    }
    assert!(
        doc.roots.len() >= 2,
        "the closing brace should remain a detached sibling line",
    );
}

#[test]
fn spans_are_present_for_all_lines() {
    let input = "a\n  b\nc\n";
    let doc = parse_generic(input);

    let mut line_count = 0usize;
    for node_id in &doc.roots {
        match doc.node(*node_id).expect("node") {
            Node::Line(line) => {
                line_count += 1;
                assert!(line.span.end_byte >= line.span.start_byte);
                assert!(line.span.line >= 1);
            }
            Node::Block(block) => {
                line_count += 1;
                assert!(block.header.span.end_byte >= block.header.span.start_byte);
                assert!(block.header.span.line >= 1);
                for child_id in &block.children {
                    if let Node::Line(child) = doc.node(*child_id).expect("child") {
                        line_count += 1;
                        assert!(child.span.end_byte >= child.span.start_byte);
                        assert!(child.span.line >= 1);
                    }
                }
            }
        }
    }

    assert_eq!(line_count, doc.metadata.line_count);
}
