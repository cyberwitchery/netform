//! a Junos multi-line quoted value is opaque text, so its body neither opens
//! nor closes blocks (see `junos_literal_region`).

use netform_dialect_junos::parse_junos;
use netform_ir::{Document, LineNode, Node, NodeId, TriviaKind};

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

fn literal_texts(doc: &Document) -> Vec<&str> {
    lines(doc)
        .into_iter()
        .filter(|line| line.trivia == TriviaKind::Literal)
        .map(|line| line.raw.as_str())
        .collect()
}

fn block(doc: &Document, id: NodeId) -> &netform_ir::BlockNode {
    match doc.node(id) {
        Some(Node::Block(block)) => block,
        other => panic!("expected a block, got {other:?}"),
    }
}

fn only_root(doc: &Document) -> &netform_ir::BlockNode {
    assert_eq!(
        doc.roots.len(),
        1,
        "expected a single root: {:?}",
        doc.roots
    );
    block(doc, doc.roots[0])
}

fn finding_codes(doc: &Document) -> Vec<&str> {
    doc.metadata
        .parse_findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect()
}

const BRACED_CERTIFICATE: &str = "\
security {
    certificates {
        local {
            SSL-CERT {
                certificate \"-----BEGIN CERTIFICATE-----
MIIDXTCCAkWgAwIBAgIJAKL0UG+mRkSP
hkiG9w0BBQwwDgQIabcd+/EFGH==
-----END CERTIFICATE-----\";
            }
        }
    }
}
";

#[test]
fn a_certificate_keeps_the_blocks_around_it_nested() {
    let doc = parse_junos(BRACED_CERTIFICATE);
    assert_eq!(doc.render(), BRACED_CERTIFICATE);

    let security = only_root(&doc);
    assert_eq!(security.header.raw, "security {");
    assert_eq!(
        security.footer.as_ref().map(|f| f.raw.as_str()),
        Some("}"),
        "the outermost brace closes `security`",
    );

    let certificates = block(&doc, security.children[0]);
    let local = block(&doc, certificates.children[0]);
    let cert = block(&doc, local.children[0]);
    assert_eq!(cert.header.raw, "            SSL-CERT {");
    assert_eq!(
        cert.footer.as_ref().map(|f| f.raw.as_str()),
        Some("            }"),
    );

    assert_eq!(
        cert.children.len(),
        4,
        "the opener plus the three lines of the value",
    );
}

#[test]
fn a_certificate_body_is_opaque_text() {
    let doc = parse_junos(BRACED_CERTIFICATE);

    assert_eq!(
        literal_texts(&doc),
        vec![
            "MIIDXTCCAkWgAwIBAgIJAKL0UG+mRkSP",
            "hkiG9w0BBQwwDgQIabcd+/EFGH==",
            "-----END CERTIFICATE-----\";",
        ],
    );

    for line in lines(&doc)
        .into_iter()
        .filter(|line| line.trivia == TriviaKind::Literal)
    {
        assert!(line.parsed.is_none(), "{:?}", line.raw);
        assert!(line.key_hint.is_none(), "{:?}", line.raw);
    }

    let opener = block(
        &doc,
        block(&doc, block(&doc, only_root(&doc).children[0]).children[0]).children[0],
    );
    assert_eq!(opener.header.trivia, TriviaKind::Content);
}

#[test]
fn a_column_zero_brace_inside_a_certificate_closes_nothing() {
    let cfg = "\
security {
    certificates {
        local \"-----BEGIN CERTIFICATE-----
}
};
-----END CERTIFICATE-----\";
    }
}
";
    let doc = parse_junos(cfg);
    assert_eq!(doc.render(), cfg);

    let security = only_root(&doc);
    assert_eq!(security.footer.as_ref().map(|f| f.raw.as_str()), Some("}"));
    let certificates = block(&doc, security.children[0]);
    assert_eq!(
        certificates.footer.as_ref().map(|f| f.raw.as_str()),
        Some("    }"),
    );
    assert_eq!(
        literal_texts(&doc),
        vec!["}", "};", "-----END CERTIFICATE-----\";"]
    );
    assert!(finding_codes(&doc).is_empty(), "{:?}", finding_codes(&doc));
}

#[test]
fn a_root_authentication_ssh_key_keeps_its_block() {
    let cfg = "\
system {
    root-authentication {
        ssh-rsa \"ssh-rsa AAAAB3Nz
morekeymaterial== user@host\"; ## SECRET-DATA
    }
}
";
    let doc = parse_junos(cfg);
    assert_eq!(doc.render(), cfg);

    let system = only_root(&doc);
    assert_eq!(system.footer.as_ref().map(|f| f.raw.as_str()), Some("}"));

    let root_auth = block(&doc, system.children[0]);
    assert_eq!(root_auth.header.raw, "    root-authentication {");
    assert_eq!(
        root_auth.footer.as_ref().map(|f| f.raw.as_str()),
        Some("    }"),
    );
    assert_eq!(
        literal_texts(&doc),
        vec!["morekeymaterial== user@host\"; ## SECRET-DATA"],
        "a trailing `## SECRET-DATA` does not stop the quote from closing the value",
    );
}

#[test]
fn a_set_format_announcement_continues_into_the_next_line() {
    let cfg = "\
set system host-name router-1
set system login announcement \"Authorized use only
Second line of the announcement\"
set system domain-name example.com
";
    let doc = parse_junos(cfg);
    assert_eq!(doc.render(), cfg);
    assert_eq!(doc.roots.len(), 4);

    let all = lines(&doc);
    assert_eq!(all[2].trivia, TriviaKind::Literal);
    assert!(all[2].parsed.is_none());
    assert!(all[2].key_hint.is_none());

    assert_eq!(
        all[3].key_hint.as_deref(),
        Some("set-system:domain-name"),
        "the statement after the value is keyed as usual",
    );
}

#[test]
fn a_closing_quote_on_its_own_line_ends_the_value() {
    let cfg = "\
system {
    login {
        announcement \"line one
line two
\";
    };
}
";
    let doc = parse_junos(cfg);
    assert_eq!(doc.render(), cfg);

    assert_eq!(literal_texts(&doc), vec!["line two", "\";"]);
    let login = block(&doc, only_root(&doc).children[0]);
    assert_eq!(
        login.footer.as_ref().map(|f| f.raw.as_str()),
        Some("    };"),
    );
}

#[test]
fn a_brace_on_the_terminator_line_does_not_close_its_block() {
    let cfg = "\
system {
    root-authentication {
        ssh-rsa \"ssh-rsa AAAA
BBBB user@host\"; }
}
";
    let doc = parse_junos(cfg);
    assert_eq!(doc.render(), cfg);

    let system = only_root(&doc);
    assert_eq!(system.footer.as_ref().map(|f| f.raw.as_str()), Some("}"));
    let root_auth = block(&doc, system.children[0]);
    assert_eq!(root_auth.footer, None);
}

#[test]
fn an_unterminated_quoted_value_is_not_read_as_a_value() {
    let cfg = "\
system {
    services {
        ssh \"unbalanced;
    }
}
";
    let doc = parse_junos(cfg);
    assert_eq!(doc.render(), cfg);

    assert_eq!(finding_codes(&doc), vec!["unterminated-literal-region"]);
    assert!(
        literal_texts(&doc).is_empty(),
        "an opener whose quote never returns is not entered",
    );

    let services = block(&doc, only_root(&doc).children[0]);
    assert_eq!(
        services.footer.as_ref().map(|f| f.raw.as_str()),
        Some("    }"),
        "the body stays ordinary configuration",
    );
}

#[test]
fn comment_lines_open_no_region() {
    let cfg = "\
## Last changed: 2026-08-15 by \"admin
system {
    /* the \" below is prose
     * and so is this
     */
    host-name router-1;
}
";
    let doc = parse_junos(cfg);
    assert_eq!(doc.render(), cfg);

    assert!(
        literal_texts(&doc).is_empty(),
        "a comment's odd quote count is prose, not an open value",
    );
    assert!(finding_codes(&doc).is_empty(), "{:?}", finding_codes(&doc));
}

#[test]
fn self_contained_values_parse_as_they_did() {
    let cfg = "\
interfaces {
    ge-0/0/0 {
        description \"uplink to core\";
        unit 0 {
            family inet;
        }
    }
}
";
    let doc = parse_junos(cfg);
    assert_eq!(doc.render(), cfg);
    assert!(literal_texts(&doc).is_empty());
    assert!(finding_codes(&doc).is_empty(), "{:?}", finding_codes(&doc));

    let interfaces = only_root(&doc);
    assert_eq!(
        interfaces.footer.as_ref().map(|f| f.raw.as_str()),
        Some("}"),
    );
}

#[test]
fn consecutive_values_each_get_their_own_region() {
    let cfg = "\
system {
    a \"one
end one\";
    b \"two
end two\";
}
";
    let doc = parse_junos(cfg);
    assert_eq!(doc.render(), cfg);

    assert_eq!(literal_texts(&doc), vec!["end one\";", "end two\";"]);
    let system = only_root(&doc);
    assert_eq!(system.children.len(), 4);
    assert_eq!(system.footer.as_ref().map(|f| f.raw.as_str()), Some("}"));
}
