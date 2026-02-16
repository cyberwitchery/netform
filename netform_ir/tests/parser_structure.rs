use netform_ir::{Node, TriviaKind, parse_generic};

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
