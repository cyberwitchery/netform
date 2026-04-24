//! FortiOS-oriented dialect profile for `netform_ir`.
//!
//! Fortinet FortiOS configuration uses `config`/`end` block markers for
//! sections, `edit`/`next` for entries within sections, and `set`/`unset`
//! keywords for key-value assignments.  Comments use `#`.
//!
//! # Example
//!
//! ```rust
//! use netform_dialect_fortios::parse_fortios;
//!
//! let cfg = "config system global\n    set hostname \"FortiGate\"\nend\n";
//! let doc = parse_fortios(cfg);
//! assert_eq!(doc.render(), cfg);
//! ```

use netform_ir::{
    Dialect, DialectHint, Document, ParsedLineParts, TriviaKind, classify_trivia_with_prefixes,
    parse_with_dialect, tokenize,
};

/// Dialect implementation for FortiOS configuration text.
#[derive(Debug, Default, Clone, Copy)]
pub struct FortiosDialect;

/// Parse text using [`FortiosDialect`].
pub fn parse_fortios(input: &str) -> Document {
    parse_with_dialect(input, &FortiosDialect)
}

impl Dialect for FortiosDialect {
    fn dialect_hint(&self) -> DialectHint {
        DialectHint::Named("fortios".to_string())
    }

    fn classify_trivia(&self, raw: &str) -> TriviaKind {
        classify_fortios_trivia(raw)
    }

    fn parse_parts(&self, raw: &str) -> Option<ParsedLineParts> {
        parse_fortios_parts(raw)
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
        fortios_key_hint(parsed)
    }
}

/// Classify trivia for FortiOS configs.
///
/// Lines starting with `#` (after leading whitespace) are comments;
/// blank/whitespace-only lines are blank; everything else is content.
fn classify_fortios_trivia(raw: &str) -> TriviaKind {
    classify_trivia_with_prefixes(raw, &["#"])
}

/// Tokenize a FortiOS content line.
///
/// Uses [`tokenize`] with no punctuation characters — FortiOS uses
/// whitespace-delimited tokens with quoted strings, similar to IOS-like
/// dialects.
fn parse_fortios_parts(raw: &str) -> Option<ParsedLineParts> {
    let tokens = tokenize(raw, &[]);
    let head = tokens.first()?.clone();
    let args = tokens.into_iter().skip(1).collect::<Vec<_>>();
    Some(ParsedLineParts { head, args })
}

/// Strip surrounding double-quotes from a token, if present.
fn unquote(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
}

/// Derive a stable identity key for FortiOS configuration lines.
///
/// Recognized patterns:
/// - `config <section> [<subsection>...]` → `config:<section>[:<subsection>...]`
/// - `edit <name>` → `edit:<name>` (quotes stripped)
/// - `set <field> ...` → `set:<field>` (stable across value changes)
/// - `unset <field>` → `unset:<field>`
/// - Block markers (`end`, `next`) do not get key hints.
fn fortios_key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    let parsed = parsed?;
    let head = parsed.head.as_str();
    let args = parsed.args.as_slice();

    match head {
        "config" => {
            if args.is_empty() {
                return None;
            }
            let path = args
                .iter()
                .map(|a| unquote(a))
                .collect::<Vec<_>>()
                .join(":");
            Some(format!("config:{path}"))
        }
        "edit" => args.first().map(|name| format!("edit:{}", unquote(name))),
        "set" | "unset" => args
            .first()
            .map(|field| format!("{head}:{}", unquote(field))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- trivia classification --

    #[test]
    fn fortios_hash_comment() {
        assert_eq!(classify_fortios_trivia("# comment"), TriviaKind::Comment);
    }

    #[test]
    fn fortios_indented_hash_comment() {
        assert_eq!(
            classify_fortios_trivia("    # indented comment"),
            TriviaKind::Comment,
        );
    }

    #[test]
    fn fortios_blank_line() {
        assert_eq!(classify_fortios_trivia(""), TriviaKind::Blank);
        assert_eq!(classify_fortios_trivia("   "), TriviaKind::Blank);
    }

    #[test]
    fn fortios_content_line() {
        assert_eq!(
            classify_fortios_trivia("config system global"),
            TriviaKind::Content,
        );
        assert_eq!(
            classify_fortios_trivia("    set hostname \"FGT\""),
            TriviaKind::Content,
        );
    }

    #[test]
    fn fortios_bang_is_not_comment() {
        // FortiOS does not use `!` as a comment prefix (unlike IOS).
        assert_eq!(classify_fortios_trivia("! note"), TriviaKind::Content);
    }

    // -- tokenization --

    #[test]
    fn fortios_tokenize_config_line() {
        let parsed = parse_fortios_parts("config system global").expect("should parse");
        assert_eq!(parsed.head, "config");
        assert_eq!(parsed.args, vec!["system", "global"]);
    }

    #[test]
    fn fortios_tokenize_set_quoted() {
        let parsed =
            parse_fortios_parts("    set hostname \"My FortiGate\"").expect("should parse");
        assert_eq!(parsed.head, "set");
        assert_eq!(parsed.args, vec!["hostname", "\"My FortiGate\""]);
    }

    #[test]
    fn fortios_tokenize_edit_quoted() {
        let parsed = parse_fortios_parts("    edit \"all\"").expect("should parse");
        assert_eq!(parsed.head, "edit");
        assert_eq!(parsed.args, vec!["\"all\""]);
    }

    #[test]
    fn fortios_tokenize_end() {
        let parsed = parse_fortios_parts("end").expect("should parse");
        assert_eq!(parsed.head, "end");
        assert!(parsed.args.is_empty());
    }

    #[test]
    fn fortios_tokenize_set_multivalue() {
        let parsed =
            parse_fortios_parts("    set subnet 10.0.0.0 255.255.255.0").expect("should parse");
        assert_eq!(parsed.head, "set");
        assert_eq!(parsed.args, vec!["subnet", "10.0.0.0", "255.255.255.0"]);
    }

    // -- key hints --

    fn hint(line: &str) -> Option<String> {
        let parsed = parse_fortios_parts(line);
        fortios_key_hint(parsed.as_ref())
    }

    #[test]
    fn key_hint_config_two_part() {
        assert_eq!(
            hint("config system global"),
            Some("config:system:global".into()),
        );
    }

    #[test]
    fn key_hint_config_one_part() {
        assert_eq!(
            hint("config firewall address"),
            Some("config:firewall:address".into()),
        );
    }

    #[test]
    fn key_hint_config_single_section() {
        assert_eq!(
            hint("config system interface"),
            Some("config:system:interface".into()),
        );
    }

    #[test]
    fn key_hint_edit_quoted() {
        assert_eq!(hint("    edit \"port1\""), Some("edit:port1".into()));
    }

    #[test]
    fn key_hint_edit_unquoted() {
        assert_eq!(hint("    edit 1"), Some("edit:1".into()));
    }

    #[test]
    fn key_hint_set_field() {
        assert_eq!(
            hint("    set hostname \"FGT\""),
            Some("set:hostname".into()),
        );
    }

    #[test]
    fn key_hint_set_type() {
        assert_eq!(hint("        set type ipmask"), Some("set:type".into()));
    }

    #[test]
    fn key_hint_set_multivalue() {
        assert_eq!(
            hint("    set subnet 10.0.0.0 255.255.255.0"),
            Some("set:subnet".into()),
        );
    }

    #[test]
    fn key_hint_set_quoted_param_unquotes() {
        assert_eq!(
            hint("    set \"custom-field\" value"),
            Some("set:custom-field".into()),
        );
    }

    #[test]
    fn key_hint_set_bare_no_hint() {
        // Bare "set" with no field name — shouldn't happen but must not panic.
        assert_eq!(hint("    set"), None);
    }

    #[test]
    fn key_hint_unset_field() {
        assert_eq!(hint("    unset comments"), Some("unset:comments".into()));
    }

    #[test]
    fn key_hint_unset_uuid() {
        assert_eq!(hint("        unset uuid"), Some("unset:uuid".into()));
    }

    #[test]
    fn key_hint_unset_bare_no_hint() {
        assert_eq!(hint("    unset"), None);
    }

    #[test]
    fn key_hint_set_description_quoted_value() {
        // The key hint captures the parameter name, not the value.
        assert_eq!(
            hint("        set description \"Production web server\""),
            Some("set:description".into()),
        );
    }

    #[test]
    fn key_hint_set_action() {
        assert_eq!(hint("        set action accept"), Some("set:action".into()),);
    }

    #[test]
    fn key_hint_end_no_hint() {
        assert_eq!(hint("end"), None);
    }

    #[test]
    fn key_hint_next_no_hint() {
        assert_eq!(hint("    next"), None);
    }

    #[test]
    fn key_hint_config_empty_no_hint() {
        // Bare "config" with no section — shouldn't happen in practice but
        // must not panic.
        assert_eq!(hint("config"), None);
    }

    #[test]
    fn key_hint_none_on_empty() {
        assert_eq!(fortios_key_hint(None), None);
    }

    // -- round-trip parsing --

    #[test]
    fn parse_fortios_round_trip() {
        let cfg = "\
config system global
    set hostname \"FortiGate\"
    set timezone 04
end
config firewall address
    edit \"all\"
        set uuid abc123
        set type ipmask
        set subnet 0.0.0.0 0.0.0.0
    next
    edit \"google-play\"
        set uuid def456
        set type fqdn
        set fqdn \"play.google.com\"
    next
end
";
        let doc = parse_fortios(cfg);
        assert_eq!(doc.render(), cfg);
    }

    #[test]
    fn parse_fortios_sets_named_dialect_hint() {
        let doc = parse_fortios("config system global\n    set hostname \"FGT\"\nend\n");
        assert_eq!(
            doc.metadata.dialect_hint,
            DialectHint::Named("fortios".into()),
        );
    }

    #[test]
    fn parse_fortios_with_comments() {
        let cfg = "# FortiOS configuration\nconfig system global\n    set hostname \"FGT\"\nend\n";
        let doc = parse_fortios(cfg);
        assert_eq!(doc.render(), cfg);
    }
}
