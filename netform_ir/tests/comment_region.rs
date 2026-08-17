//! parser coverage for multi-line comment regions, and for their precedence
//! over literal regions.

use netform_ir::{
    Dialect, Document, IosLikeDialect, LiteralTerminator, Node, NodeId, ParsedLineParts,
    TriviaKind, common_key_hint, parse_with_dialect,
};

/// dialect opening a comment region on any line naming `NOTE` and a literal
/// region on any line naming `TEXT`, independent of any vendor syntax.
/// `BEGIN-NOTE-TEXT` opens both at once, so the two can be ordered.
struct MarkerDialect;

impl Dialect for MarkerDialect {
    fn classify_trivia(&self, raw: &str) -> TriviaKind {
        if raw.trim().is_empty() {
            TriviaKind::Blank
        } else {
            TriviaKind::Content
        }
    }

    fn parse_parts(&self, raw: &str) -> Option<ParsedLineParts> {
        Some(ParsedLineParts {
            head: raw.trim().to_string(),
            args: Vec::new(),
        })
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
        Some(parsed?.head.clone())
    }

    fn literal_region(&self, raw: &str) -> Option<LiteralTerminator> {
        raw.contains("TEXT")
            .then(|| LiteralTerminator::ExactLine("END".to_string()))
    }

    fn comment_region(&self, raw: &str) -> Option<LiteralTerminator> {
        raw.contains("NOTE")
            .then(|| LiteralTerminator::ExactLine("END".to_string()))
    }
}

fn line_kinds(doc: &Document) -> Vec<(&str, TriviaKind)> {
    fn walk<'a>(doc: &'a Document, id: NodeId, out: &mut Vec<(&'a str, TriviaKind)>) {
        match doc.node(id).expect("node in arena") {
            Node::Line(line) => out.push((line.raw.as_str(), line.trivia)),
            Node::Block(block) => {
                out.push((block.header.raw.as_str(), block.header.trivia));
                for child in &block.children {
                    walk(doc, *child, out);
                }
                if let Some(footer) = &block.footer {
                    out.push((footer.raw.as_str(), footer.trivia));
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

fn finding_codes(doc: &Document) -> Vec<&str> {
    doc.metadata
        .parse_findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect()
}

#[test]
fn region_body_and_terminator_are_comments() {
    let input = "BEGIN-NOTE\nprose\n  indented prose\nEND\nhostname edge-1\n";
    let doc = parse_with_dialect(input, &MarkerDialect);

    assert_eq!(doc.render(), input);
    assert_eq!(
        line_kinds(&doc),
        vec![
            ("BEGIN-NOTE", TriviaKind::Content),
            ("prose", TriviaKind::Comment),
            ("  indented prose", TriviaKind::Comment),
            ("END", TriviaKind::Comment),
            ("hostname edge-1", TriviaKind::Content),
        ],
    );
}

#[test]
fn a_comment_body_is_never_tokenized_or_key_hinted() {
    let input = "BEGIN-NOTE\nhostname ghost\nEND\n";
    let doc = parse_with_dialect(input, &MarkerDialect);

    let Some(Node::Line(body)) = doc.node(doc.roots[1]) else {
        panic!("expected a body line");
    };
    assert_eq!(body.raw, "hostname ghost");
    assert!(body.parsed.is_none());
    assert!(body.key_hint.is_none());
}

#[test]
fn a_dedented_comment_body_does_not_close_the_block_around_it() {
    let input = "outer\n  inner\n    BEGIN-NOTE\nprose at column zero\nEND\n    sibling\n";
    let doc = parse_with_dialect(input, &MarkerDialect);

    assert_eq!(doc.render(), input);
    assert_eq!(doc.roots.len(), 1);

    let Some(Node::Block(outer)) = doc.node(doc.roots[0]) else {
        panic!("expected an `outer` block");
    };
    let Some(Node::Block(inner)) = doc.node(outer.children[0]) else {
        panic!("expected an `inner` block");
    };
    assert_eq!(
        inner.children.len(),
        4,
        "the comment region's three lines plus the sibling stay inside `inner`",
    );
}

#[test]
fn a_comment_region_wins_over_a_literal_region_on_the_same_line() {
    let input = "BEGIN-NOTE-TEXT\nbody\nEND\n";
    let doc = parse_with_dialect(input, &MarkerDialect);

    assert_eq!(
        line_kinds(&doc),
        vec![
            ("BEGIN-NOTE-TEXT", TriviaKind::Content),
            ("body", TriviaKind::Comment),
            ("END", TriviaKind::Comment),
        ],
    );
}

#[test]
fn a_comment_body_cannot_open_a_literal_region() {
    let input = "BEGIN-NOTE\nBEGIN-TEXT\nEND\nhostname edge-1\n";
    let doc = parse_with_dialect(input, &MarkerDialect);

    assert!(
        line_kinds(&doc)
            .iter()
            .all(|(_, trivia)| *trivia != TriviaKind::Literal),
        "{:?}",
        line_kinds(&doc),
    );
    assert!(finding_codes(&doc).is_empty(), "{:?}", finding_codes(&doc));
}

#[test]
fn an_unterminated_comment_region_is_not_entered() {
    let input = "BEGIN-NOTE\nhostname edge-1\n";
    let doc = parse_with_dialect(input, &MarkerDialect);

    assert_eq!(doc.render(), input);
    assert_eq!(finding_codes(&doc), vec!["unterminated-comment-region"]);
    assert_eq!(
        line_kinds(&doc),
        vec![
            ("BEGIN-NOTE", TriviaKind::Content),
            ("hostname edge-1", TriviaKind::Content),
        ],
    );
}

#[test]
fn a_comment_body_mixing_tabs_and_spaces_is_not_flagged() {
    let doc = parse_with_dialect("BEGIN-NOTE\n \tprose\nEND\n", &MarkerDialect);
    assert!(finding_codes(&doc).is_empty(), "{:?}", finding_codes(&doc));

    let doc = parse_with_dialect("outer\n \thostname edge-1\n", &MarkerDialect);
    assert_eq!(finding_codes(&doc), vec!["mixed-leading-whitespace"]);
}

#[test]
fn a_dialect_without_a_comment_region_is_unaffected() {
    let ios_like: IosLikeDialect = IosLikeDialect::new("iosxe", common_key_hint);
    let input = "banner motd ^C\nAuthorized use only\n^C\nhostname edge-1\n";
    let doc = parse_with_dialect(input, &ios_like);

    assert_eq!(
        line_kinds(&doc),
        vec![
            ("banner motd ^C", TriviaKind::Content),
            ("Authorized use only", TriviaKind::Literal),
            ("^C", TriviaKind::Literal),
            ("hostname edge-1", TriviaKind::Content),
        ],
    );
}
