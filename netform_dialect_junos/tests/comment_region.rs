//! a Junos `/* … */` comment is prose, so nothing in its body is read as
//! configuration (see `junos_comment_region`).

use netform_dialect_junos::parse_junos;
use netform_ir::{Document, LineNode, Node, TriviaKind};

fn lines(doc: &Document) -> Vec<&LineNode> {
    let mut out = Vec::new();
    for node in &doc.arena {
        match node {
            Node::Line(line) => out.push(line),
            Node::Block(block) => {
                out.push(&block.header);
                out.extend(block.footer.as_ref());
            }
        }
    }
    out
}

fn trivia_kinds(doc: &Document, kind: TriviaKind) -> Vec<&str> {
    lines(doc)
        .into_iter()
        .filter(|line| line.trivia == kind)
        .map(|line| line.raw.as_str())
        .collect()
}

fn key_hints(doc: &Document) -> Vec<&str> {
    lines(doc)
        .into_iter()
        .filter_map(|line| line.key_hint.as_deref())
        .collect()
}

fn finding_codes(doc: &Document) -> Vec<&str> {
    doc.metadata
        .parse_findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect()
}

/// a realistic configuration whose comment prose carries an inch mark and which
/// later holds an ordinary quoted value.
const RACK_NOTE: &str = "\
## Last changed: 2026-08-15 12:00:00 UTC
/* site notes
   Rack 4, 19\" cabinet, row B
   contact ops@example.com
 */
system {
    host-name router-1;
}
security {
    zones {
        security-zone TRUST {
            interfaces {
                ge-0/0/0.0;
            }
        }
    }
}
interfaces {
    ge-0/0/0 {
        description \"uplink to core\";
    }
}
protocols {
    bgp {
        group PEERS {
            type external;
        }
    }
}
";

#[test]
fn an_inch_mark_in_comment_prose_parses_as_the_same_document_as_plain_text() {
    let with_inch_mark = parse_junos(RACK_NOTE);
    let control = parse_junos(&RACK_NOTE.replace("19\" cabinet", "19in cabinet"));

    assert_eq!(with_inch_mark.render(), RACK_NOTE);
    assert_eq!(with_inch_mark.roots.len(), control.roots.len());
    assert_eq!(key_hints(&with_inch_mark), key_hints(&control));
    assert_eq!(
        key_hints(&with_inch_mark),
        vec![
            "system",
            "security",
            "interfaces",
            "interfaces",
            "protocols"
        ],
    );
    assert!(
        trivia_kinds(&with_inch_mark, TriviaKind::Literal).is_empty(),
        "{:?}",
        trivia_kinds(&with_inch_mark, TriviaKind::Literal),
    );
    assert!(
        finding_codes(&with_inch_mark).is_empty(),
        "{:?}",
        finding_codes(&with_inch_mark),
    );
}

#[test]
fn comment_body_lines_are_comments() {
    let doc = parse_junos(RACK_NOTE);

    assert_eq!(
        trivia_kinds(&doc, TriviaKind::Comment),
        vec![
            "## Last changed: 2026-08-15 12:00:00 UTC",
            "/* site notes",
            "   Rack 4, 19\" cabinet, row B",
            "   contact ops@example.com",
            " */",
        ],
    );

    for line in lines(&doc)
        .into_iter()
        .filter(|line| line.trivia == TriviaKind::Comment)
    {
        assert!(line.parsed.is_none(), "{:?}", line.raw);
        assert!(line.key_hint.is_none(), "{:?}", line.raw);
    }
}

#[test]
fn an_odd_quote_in_comment_prose_files_no_finding() {
    let cfg = "\
/* site notes
   the 19\" rack was reviewed by ops
 */
system {
    host-name router-1;
}
";
    let doc = parse_junos(cfg);

    assert_eq!(doc.render(), cfg);
    assert!(finding_codes(&doc).is_empty(), "{:?}", finding_codes(&doc));
}

#[test]
fn indented_comment_prose_does_not_close_the_block_around_it() {
    let cfg = "\
interfaces {
    ge-0/0/0 {
        /* uplink notes
   patched to panel 3
 */
        description \"uplink to core\";
    }
}
";
    let doc = parse_junos(cfg);
    assert_eq!(doc.render(), cfg);
    assert!(finding_codes(&doc).is_empty(), "{:?}", finding_codes(&doc));
    assert_eq!(doc.roots.len(), 1);

    let Some(Node::Block(interfaces)) = doc.node(doc.roots[0]) else {
        panic!("expected an interfaces block");
    };
    let Some(Node::Block(port)) = doc.node(interfaces.children[0]) else {
        panic!("expected a ge-0/0/0 block");
    };
    assert_eq!(
        port.footer.as_ref().map(|f| f.raw.as_str()),
        Some("    }"),
        "the dedented prose must not have closed ge-0/0/0 early",
    );
    assert_eq!(
        port.children.len(),
        4,
        "three comment lines and the description",
    );
}

#[test]
fn a_brace_inside_comment_prose_opens_no_block() {
    let cfg = "\
/* an example
   interfaces {
 */
system {
    host-name router-1;
}
";
    let doc = parse_junos(cfg);
    assert_eq!(doc.render(), cfg);
    assert_eq!(doc.roots.len(), 4, "three comment lines and `system`");
    assert!(key_hints(&doc) == vec!["system"], "{:?}", key_hints(&doc));
}

#[test]
fn a_single_line_comment_opens_no_region() {
    let cfg = "\
/* uplink to the core router */
interfaces {
    ge-0/0/0 {
        description \"uplink to core\";
    }
}
";
    let doc = parse_junos(cfg);
    assert_eq!(doc.render(), cfg);
    assert_eq!(key_hints(&doc), vec!["interfaces"]);
    assert!(finding_codes(&doc).is_empty(), "{:?}", finding_codes(&doc));
}

#[test]
fn an_unterminated_comment_is_not_entered() {
    let cfg = "\
/* site notes
system {
    host-name router-1;
}
";
    let doc = parse_junos(cfg);
    assert_eq!(doc.render(), cfg);

    assert_eq!(finding_codes(&doc), vec!["unterminated-comment-region"]);
    assert_eq!(
        key_hints(&doc),
        vec!["system"],
        "the rest of the file stays configuration",
    );
}

#[test]
fn a_quoted_value_after_a_comment_still_opens_its_own_region() {
    let cfg = "\
/* the key below is prose-adjacent
   rotated 2026-08-01
 */
system {
    root-authentication {
        ssh-rsa \"ssh-rsa AAAAB3Nz
morekeymaterial== user@host\";
    }
}
";
    let doc = parse_junos(cfg);
    assert_eq!(doc.render(), cfg);

    assert_eq!(
        trivia_kinds(&doc, TriviaKind::Literal),
        vec!["morekeymaterial== user@host\";"],
    );
    assert!(finding_codes(&doc).is_empty(), "{:?}", finding_codes(&doc));
}
