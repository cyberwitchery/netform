//! Parser coverage for multi-line literal regions (IOS-family banners).

use netform_ir::{
    Dialect, IosLikeDialect, LiteralTerminator, Node, ParsedLineParts, TriviaKind, common_key_hint,
    parse_generic, parse_with_dialect,
};

const IOS_LIKE: IosLikeDialect = IosLikeDialect::new("iosxe", common_key_hint);

/// dialect whose only customization is a literal region, so region handling is
/// exercised independently of any vendor banner syntax.
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
fn region_body_and_terminator_are_literal() {
    let input = "banner motd ^C\nAuthorized use only\n^C\nhostname edge-1\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    assert_eq!(
        line_kinds(&doc),
        vec![
            ("banner motd ^C".to_string(), TriviaKind::Content),
            ("Authorized use only".to_string(), TriviaKind::Literal),
            ("^C".to_string(), TriviaKind::Literal),
            ("hostname edge-1".to_string(), TriviaKind::Content),
        ],
    );
}

#[test]
fn region_exits_at_the_terminator_so_later_lines_parse_normally() {
    let input = "banner motd ^C\ntext\n^C\ninterface GigabitEthernet0/0/0\n  description WAN\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    let block = doc
        .roots
        .iter()
        .filter_map(|id| match doc.node(*id) {
            Some(Node::Block(block)) => Some(block),
            _ => None,
        })
        .next()
        .expect("interface block after the region");
    assert_eq!(block.header.raw, "interface GigabitEthernet0/0/0");
    assert_eq!(block.children.len(), 1);
}

#[test]
fn body_line_that_looks_like_a_comment_is_not_a_comment() {
    let input = "banner motd ^C\n! not a comment\n# nor this\n^C\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    assert_eq!(trivia_of(&doc, "! not a comment"), TriviaKind::Literal);
    assert_eq!(trivia_of(&doc, "# nor this"), TriviaKind::Literal);
}

#[test]
fn blank_body_line_is_literal_not_blank() {
    let input = "banner motd ^C\nline one\n\nline two\n^C\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    assert_eq!(trivia_of(&doc, ""), TriviaKind::Literal);
}

#[test]
fn body_lines_carry_no_tokenization_or_key_hint() {
    let input = "banner motd ^C\ninterface GigabitEthernet0/0/0\n^C\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    let body = doc
        .roots
        .iter()
        .filter_map(|id| match doc.node(*id) {
            Some(Node::Line(line)) if line.raw == "interface GigabitEthernet0/0/0" => Some(line),
            _ => None,
        })
        .next()
        .expect("banner body line");

    assert_eq!(body.trivia, TriviaKind::Literal);
    assert_eq!(body.parsed, None);
    assert_eq!(body.key_hint, None);
}

#[test]
fn indented_body_lines_do_not_open_a_block() {
    let input = "banner motd ^C\nNotice:\n    indented banner art\n^C\nhostname edge-1\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    assert!(
        doc.arena.iter().all(|node| matches!(node, Node::Line(_))),
        "no banner body line may open a block",
    );
    assert_eq!(doc.roots.len(), 5);
}

#[test]
fn body_lines_do_not_close_an_enclosing_block() {
    let input =
        "interface Vlan10\n  description mgmt\n  banner motd ^C\nflush left\n^C\n  no shutdown\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    assert_eq!(doc.roots.len(), 1);
    match doc.node(doc.roots[0]).expect("root block") {
        Node::Block(block) => {
            assert_eq!(block.header.raw, "interface Vlan10");
            assert_eq!(block.children.len(), 5);
        }
        _ => panic!("expected the interface block to hold the whole region"),
    }
}

#[test]
fn one_line_banner_whose_text_contains_a_space_opens_no_region() {
    for (motd, login) in [
        ("banner motd #Hello world#", "banner login #Second#"),
        ("banner motd ^CHello world^C", "banner login ^CSecond^C"),
    ] {
        let input = format!(
            "{motd}\ninterface GigabitEthernet0/0/0\n  ! WAN side\n  description uplink\n{login}\n"
        );
        let doc = parse_with_dialect(&input, &IOS_LIKE);

        assert_eq!(
            line_kinds(&doc),
            vec![
                (motd.to_string(), TriviaKind::Content),
                (
                    "interface GigabitEthernet0/0/0".to_string(),
                    TriviaKind::Content
                ),
                ("  ! WAN side".to_string(), TriviaKind::Comment),
                ("  description uplink".to_string(), TriviaKind::Content),
                (login.to_string(), TriviaKind::Content),
            ],
            "{motd}",
        );
        assert!(doc.metadata.parse_findings.is_empty(), "{motd}");

        let block = doc
            .roots
            .iter()
            .filter_map(|id| match doc.node(*id) {
                Some(Node::Block(block)) => Some(block),
                _ => None,
            })
            .next()
            .expect("interface block below the banner");
        assert_eq!(block.header.raw, "interface GigabitEthernet0/0/0");
        assert_eq!(block.children.len(), 2, "{motd}");
    }
}

#[test]
fn unterminated_region_is_declined_and_reported() {
    let input = "banner motd ^C\ninterface GigabitEthernet0/0/0\n  description WAN\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    assert!(
        line_kinds(&doc)
            .iter()
            .all(|(_, trivia)| *trivia != TriviaKind::Literal),
        "an unterminated region must not swallow the rest of the file",
    );

    let finding = doc
        .metadata
        .parse_findings
        .iter()
        .find(|finding| finding.code == "unterminated-literal-region")
        .expect("unterminated-literal-region finding");
    assert_eq!(finding.span.line, 1);
    assert!(finding.message.contains("^C"));
}

#[test]
fn terminated_region_reports_no_finding() {
    let doc = parse_with_dialect("banner motd ^C\ntext\n^C\n", &IOS_LIKE);
    assert!(doc.metadata.parse_findings.is_empty());
}

#[test]
fn self_contained_banner_opens_no_region() {
    let input = "banner motd ^CHi^C\n! real comment\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    assert_eq!(trivia_of(&doc, "! real comment"), TriviaKind::Comment);
    assert!(doc.metadata.parse_findings.is_empty());
}

#[test]
fn punctuation_delimiter_glued_to_the_text_opens_a_region() {
    let input = "banner motd #Warning restricted\n! Authorized use only\n#\nhostname edge-1\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    assert_eq!(
        line_kinds(&doc),
        vec![
            (
                "banner motd #Warning restricted".to_string(),
                TriviaKind::Content
            ),
            ("! Authorized use only".to_string(), TriviaKind::Literal),
            ("#".to_string(), TriviaKind::Literal),
            ("hostname edge-1".to_string(), TriviaKind::Content),
        ],
    );
    assert!(doc.metadata.parse_findings.is_empty());
}

#[test]
fn word_delimiter_is_not_split_by_its_first_character() {
    let input = "banner motd EOF\nEnd of the line\nEOF\n! real comment\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    assert_eq!(
        line_kinds(&doc),
        vec![
            ("banner motd EOF".to_string(), TriviaKind::Content),
            ("End of the line".to_string(), TriviaKind::Literal),
            ("EOF".to_string(), TriviaKind::Literal),
            ("! real comment".to_string(), TriviaKind::Comment),
        ],
    );
}

#[test]
fn delimiter_less_eos_form_ends_at_eof_marker() {
    let input = "banner motd\nWelcome to edge-1\nEOF\n! real comment\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    assert_eq!(trivia_of(&doc, "Welcome to edge-1"), TriviaKind::Literal);
    assert_eq!(trivia_of(&doc, "EOF"), TriviaKind::Literal);
    assert_eq!(trivia_of(&doc, "! real comment"), TriviaKind::Comment);
}

#[test]
fn consecutive_regions_are_tracked_independently() {
    let input = "banner motd ^C\nfirst\n^C\nbanner login ^C\nsecond\n^C\nhostname edge-1\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    assert_eq!(trivia_of(&doc, "first"), TriviaKind::Literal);
    assert_eq!(trivia_of(&doc, "second"), TriviaKind::Literal);
    assert_eq!(trivia_of(&doc, "banner login ^C"), TriviaKind::Content);
    assert_eq!(trivia_of(&doc, "hostname edge-1"), TriviaKind::Content);
}

#[test]
fn a_banner_line_inside_a_region_does_not_open_a_nested_region() {
    let input = "banner motd ^C\nbanner login #\ntext\n^C\nhostname edge-1\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    assert_eq!(trivia_of(&doc, "banner login #"), TriviaKind::Literal);
    assert_eq!(trivia_of(&doc, "hostname edge-1"), TriviaKind::Content);
    assert!(doc.metadata.parse_findings.is_empty());
}

#[test]
fn mixed_leading_whitespace_in_a_region_is_not_reported() {
    let input = "banner motd ^C\n \tascii art\n^C\n";
    let doc = parse_with_dialect(input, &IOS_LIKE);

    assert!(doc.metadata.parse_findings.is_empty());
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

#[test]
fn render_round_trips_documents_containing_regions() {
    let inputs = [
        "banner motd ^C\nAuthorized use only\n! looks like a comment\n^C\nhostname edge-1\n",
        "banner motd ^C\r\nmixed endings\r\n^C\nhostname edge-1",
        "banner motd\nEOS style\nEOF\n",
        "banner motd ^C\nnever closed\ninterface Vlan1\n",
        "interface Vlan10\n  description mgmt\n  banner motd ^C\nflush left\n^C\n  no shutdown\n",
        "banner motd ^C\n\n\n^C\n",
    ];

    for input in inputs {
        let doc = parse_with_dialect(input, &IOS_LIKE);
        assert_eq!(doc.render(), input, "round trip failed for {input:?}");
        assert_eq!(doc.metadata.line_count, input.lines().count().max(1));
    }
}
