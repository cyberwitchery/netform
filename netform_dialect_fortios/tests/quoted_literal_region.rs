//! a FortiOS multi-line quoted value is opaque free text, not configuration
//! (see `fortios_literal_region`).

use netform_dialect_fortios::parse_fortios;
use netform_ir::{Document, Node, NodeId, TriviaKind};

const CERTIFICATE: &str = "\
config vpn certificate local
    edit \"Fortinet_CA\"
        set private-key \"-----BEGIN ENCRYPTED PRIVATE KEY-----
MIIFDjBABgkqhkiG9w0BBQ0wMzAbBgkq
hkiG9w0BBQwwDgQIabcd+/EFGH==
-----END ENCRYPTED PRIVATE KEY-----\"
        set range global
    next
end
";

fn line_kinds(doc: &Document) -> Vec<(String, TriviaKind)> {
    fn walk(doc: &Document, id: NodeId, out: &mut Vec<(String, TriviaKind)>) {
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

fn block(doc: &Document, id: NodeId) -> &netform_ir::BlockNode {
    match doc.node(id).expect("node in arena") {
        Node::Block(block) => block,
        Node::Line(line) => panic!("expected a block, found the line {:?}", line.raw),
    }
}

fn line(doc: &Document, id: NodeId) -> &netform_ir::LineNode {
    match doc.node(id).expect("node in arena") {
        Node::Line(line) => line,
        Node::Block(block) => panic!("expected a line, found the block {:?}", block.header.raw),
    }
}

#[test]
fn certificate_body_stays_inside_the_edit_block() {
    let doc = parse_fortios(CERTIFICATE);

    assert_eq!(doc.roots.len(), 1);
    let config = block(&doc, doc.roots[0]);
    assert_eq!(config.header.raw, "config vpn certificate local");
    assert_eq!(config.footer.as_ref().map(|f| f.raw.as_str()), Some("end"));
    assert_eq!(config.children.len(), 1);

    let edit = block(&doc, config.children[0]);
    assert_eq!(edit.header.raw, "    edit \"Fortinet_CA\"");
    assert_eq!(
        edit.footer.as_ref().map(|f| f.raw.as_str()),
        Some("    next"),
    );
    assert_eq!(
        edit.children
            .iter()
            .map(|id| line(&doc, *id).raw.as_str())
            .collect::<Vec<_>>(),
        vec![
            "        set private-key \"-----BEGIN ENCRYPTED PRIVATE KEY-----",
            "MIIFDjBABgkqhkiG9w0BBQ0wMzAbBgkq",
            "hkiG9w0BBQwwDgQIabcd+/EFGH==",
            "-----END ENCRYPTED PRIVATE KEY-----\"",
            "        set range global",
        ],
    );
    assert!(doc.metadata.parse_findings.is_empty());
}

#[test]
fn opener_stays_content_and_body_lines_are_literal() {
    let doc = parse_fortios(CERTIFICATE);

    assert_eq!(
        line_kinds(&doc),
        vec![
            (
                "config vpn certificate local".to_string(),
                TriviaKind::Content
            ),
            ("    edit \"Fortinet_CA\"".to_string(), TriviaKind::Content),
            (
                "        set private-key \"-----BEGIN ENCRYPTED PRIVATE KEY-----".to_string(),
                TriviaKind::Content,
            ),
            (
                "MIIFDjBABgkqhkiG9w0BBQ0wMzAbBgkq".to_string(),
                TriviaKind::Literal,
            ),
            (
                "hkiG9w0BBQwwDgQIabcd+/EFGH==".to_string(),
                TriviaKind::Literal,
            ),
            (
                "-----END ENCRYPTED PRIVATE KEY-----\"".to_string(),
                TriviaKind::Literal,
            ),
            ("        set range global".to_string(), TriviaKind::Content),
            ("    next".to_string(), TriviaKind::Content),
            ("end".to_string(), TriviaKind::Content),
        ],
    );
}

#[test]
fn body_lines_carry_no_tokenization_or_key_hint() {
    let doc = parse_fortios(CERTIFICATE);

    let mut checked = 0usize;
    for node in &doc.arena {
        let Node::Line(body) = node else { continue };
        if body.trivia != TriviaKind::Literal {
            continue;
        }
        assert_eq!(body.parsed, None, "{:?}", body.raw);
        assert_eq!(body.key_hint, None, "{:?}", body.raw);
        checked += 1;
    }
    assert_eq!(checked, 3);
}

#[test]
fn round_trip_stays_byte_identical() {
    assert_eq!(parse_fortios(CERTIFICATE).render(), CERTIFICATE);
}

#[test]
fn a_body_line_reading_end_or_next_does_not_close_a_block() {
    let cfg = "\
config system replacemsg admin \"pre_admin-disclaimer-text\"
    set buffer \"Access to this device is restricted.
end
next
Session logging is enabled.\"
    set header http
end
config system global
    set hostname \"FortiGate\"
end
";
    let doc = parse_fortios(cfg);

    assert_eq!(doc.render(), cfg);
    assert_eq!(doc.roots.len(), 2, "{:?}", line_kinds(&doc));

    let replacemsg = block(&doc, doc.roots[0]);
    assert_eq!(
        replacemsg.footer.as_ref().map(|f| f.raw.as_str()),
        Some("end"),
        "only the flush-left `end` below the value closes the block",
    );
    assert_eq!(
        replacemsg
            .children
            .iter()
            .map(|id| line(&doc, *id).raw.as_str())
            .collect::<Vec<_>>(),
        vec![
            "    set buffer \"Access to this device is restricted.",
            "end",
            "next",
            "Session logging is enabled.\"",
            "    set header http",
        ],
    );

    let global = block(&doc, doc.roots[1]);
    assert_eq!(global.header.raw, "config system global");
}

#[test]
fn a_body_line_starting_with_a_hash_is_not_a_comment() {
    let cfg = "\
config system replacemsg webproxy \"deny\"
    set buffer \"<style>
#banner { color: red; }
</style>\"
end
";
    let doc = parse_fortios(cfg);

    assert_eq!(doc.render(), cfg);
    let hash = line_kinds(&doc)
        .into_iter()
        .find(|(raw, _)| raw == "#banner { color: red; }")
        .expect("the style rule survives");
    assert_eq!(hash.1, TriviaKind::Literal);
}

#[test]
fn a_blank_body_line_is_literal_not_blank() {
    let cfg = "\
config system replacemsg admin \"pre_admin-disclaimer-text\"
    set buffer \"top

bottom\"
end
";
    let doc = parse_fortios(cfg);

    assert_eq!(doc.render(), cfg);
    assert!(
        line_kinds(&doc)
            .iter()
            .any(|(raw, kind)| raw.is_empty() && *kind == TriviaKind::Literal),
    );
}

#[test]
fn escaped_quotes_in_the_body_do_not_close_the_region() {
    let cfg = "\
config system replacemsg webproxy \"deny\"
    set buffer \"<html>
<a href=\\\"https://example.net/help\\\">Help</a>
</html>\"
    set header http
end
";
    let doc = parse_fortios(cfg);

    assert_eq!(doc.render(), cfg);
    assert_eq!(
        line_kinds(&doc)
            .into_iter()
            .filter(|(_, kind)| *kind == TriviaKind::Literal)
            .map(|(raw, _)| raw)
            .collect::<Vec<_>>(),
        vec![
            "<a href=\\\"https://example.net/help\\\">Help</a>".to_string(),
            "</html>\"".to_string(),
        ],
    );
}

#[test]
fn escaped_quotes_on_the_opener_line_still_open_a_region() {
    let cfg = "\
config system replacemsg webproxy \"deny\"
    set buffer \"<a href=\\\"/help\\\">
Help</a>\"
end
";
    let doc = parse_fortios(cfg);

    assert_eq!(doc.render(), cfg);
    assert_eq!(
        line_kinds(&doc)
            .into_iter()
            .filter(|(_, kind)| *kind == TriviaKind::Literal)
            .map(|(raw, _)| raw)
            .collect::<Vec<_>>(),
        vec!["Help</a>\"".to_string()],
    );
}

#[test]
fn an_escaped_backslash_before_the_quote_closes_the_region() {
    let cfg = "\
config system global
    set comment \"first
a trailing backslash \\\\\"
    set hostname \"FortiGate\"
end
";
    let doc = parse_fortios(cfg);

    assert_eq!(doc.render(), cfg);
    assert_eq!(
        line_kinds(&doc)
            .into_iter()
            .filter(|(_, kind)| *kind == TriviaKind::Literal)
            .map(|(raw, _)| raw)
            .collect::<Vec<_>>(),
        vec!["a trailing backslash \\\\\"".to_string()],
    );
}

#[test]
fn single_line_quoted_values_open_no_region() {
    let cfg = "\
config firewall address
    edit \"all\"
        set uuid abc123
        set comment \"he said \\\"hi\\\"\"
    next
end
";
    let doc = parse_fortios(cfg);

    assert_eq!(doc.render(), cfg);
    assert!(
        line_kinds(&doc)
            .iter()
            .all(|(_, kind)| *kind != TriviaKind::Literal),
        "{:?}",
        line_kinds(&doc),
    );
    assert!(doc.metadata.parse_findings.is_empty());
}

#[test]
fn an_unterminated_value_is_reported_and_left_as_configuration() {
    let cfg = "\
config system global
    set comment \"never closed
    set timezone 04
end
";
    let doc = parse_fortios(cfg);

    assert_eq!(doc.render(), cfg);
    let findings = &doc.metadata.parse_findings;
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].code, "unterminated-literal-region");
    assert!(findings[0].message.contains('"'), "{}", findings[0].message,);
    assert_eq!(findings[0].span.line, 2);
    assert!(
        line_kinds(&doc)
            .iter()
            .all(|(_, kind)| *kind != TriviaKind::Literal),
    );
}

#[test]
fn an_unterminated_value_runs_to_the_next_quoted_line() {
    let cfg = "\
config system global
    set comment \"never closed
    set timezone 04
    set hostname \"FortiGate\"
end
";
    let doc = parse_fortios(cfg);

    assert_eq!(doc.render(), cfg);
    assert!(doc.metadata.parse_findings.is_empty());
    assert_eq!(
        line_kinds(&doc)
            .into_iter()
            .filter(|(_, kind)| *kind == TriviaKind::Literal)
            .map(|(raw, _)| raw)
            .collect::<Vec<_>>(),
        vec![
            "    set timezone 04".to_string(),
            "    set hostname \"FortiGate\"".to_string(),
        ],
    );
}
