//! Junos-oriented dialect profile for `netform_ir`.
//!
//! This crate provides a conservative Junos profile that customizes:
//! - comment classification (`#`, `/*`, `*`, `*/`)
//! - line tokenization for braces/semicolons and quoted strings
//!
//! # Example
//!
//! ```rust
//! use netform_dialect_junos::parse_junos;
//!
//! let cfg = "interfaces {\n    ge-0/0/0 {\n        disable;\n    }\n}\n";
//! let doc = parse_junos(cfg);
//! assert_eq!(doc.render(), cfg);
//! ```

use netform_ir::{Dialect, DialectHint, Document, ParsedLineParts, TriviaKind, parse_with_dialect};

/// Dialect implementation for Junos-like configuration text.
#[derive(Debug, Default, Clone, Copy)]
pub struct JunosDialect;

/// Parse text using [`JunosDialect`].
pub fn parse_junos(input: &str) -> Document {
    parse_with_dialect(input, &JunosDialect)
}

impl Dialect for JunosDialect {
    fn dialect_hint(&self) -> DialectHint {
        DialectHint::Named("junos".to_string())
    }

    fn classify_trivia(&self, raw: &str) -> TriviaKind {
        classify_junos_trivia(raw)
    }

    fn parse_parts(&self, raw: &str) -> Option<ParsedLineParts> {
        parse_junos_parts(raw)
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
        junos_key_hint(parsed)
    }
}

fn classify_junos_trivia(raw: &str) -> TriviaKind {
    if raw.trim().is_empty() {
        return TriviaKind::Blank;
    }

    let trimmed = raw.trim_start();
    if trimmed.starts_with('#')
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with("*/")
    {
        return TriviaKind::Comment;
    }

    TriviaKind::Content
}

fn parse_junos_parts(raw: &str) -> Option<ParsedLineParts> {
    let tokens = tokenize_junos(raw);
    let head = tokens.first()?.clone();
    let args = tokens.into_iter().skip(1).collect::<Vec<_>>();
    Some(ParsedLineParts { head, args })
}

fn tokenize_junos(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;
    let mut escape = false;

    for ch in raw.chars() {
        if let Some(q) = in_quote {
            if escape {
                current.push(ch);
                escape = false;
                continue;
            }

            if ch == '\\' {
                current.push(ch);
                escape = true;
                continue;
            }

            current.push(ch);
            if ch == q {
                in_quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => {
                current.push(ch);
                in_quote = Some(ch);
            }
            '{' | '}' | ';' => {
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                }
                current.clear();
                tokens.push(ch.to_string());
            }
            c if c.is_whitespace() => {
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.trim().is_empty() {
        tokens.push(current.trim().to_string());
    }

    tokens
}

fn junos_key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    let parsed = parsed?;
    let head = parsed.head.as_str();
    let args = parsed.args.as_slice();

    match head {
        "interfaces" | "protocols" | "routing-instances" | "policy-options" => {
            Some(head.to_string())
        }
        "set" => set_style_key_hint(args),
        _ => None,
    }
}

fn set_style_key_hint(args: &[String]) -> Option<String> {
    match args {
        [section, name, ..] if section == "interfaces" => Some(format!("set-interface:{name}")),
        [section, name, ..] if section == "routing-instances" => {
            Some(format!("set-routing-instance:{name}"))
        }
        [section, proto, asn, ..] if section == "protocols" && proto == "bgp" => {
            Some(format!("set-protocols:bgp:{asn}"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junos_comment_classification_supports_hash_and_block_styles() {
        assert_eq!(classify_junos_trivia("# note"), TriviaKind::Comment);
        assert_eq!(classify_junos_trivia("/* note */"), TriviaKind::Comment);
        assert_eq!(classify_junos_trivia("*/"), TriviaKind::Comment);
        assert_eq!(classify_junos_trivia("interfaces {"), TriviaKind::Content);
    }

    #[test]
    fn junos_tokenization_keeps_brace_and_semicolon_tokens() {
        let parsed = parse_junos_parts("interfaces {").expect("content should parse");
        assert_eq!(parsed.head, "interfaces");
        assert_eq!(parsed.args, vec!["{"]);

        let parsed =
            parse_junos_parts("description \"Uplink to core\";").expect("content should parse");
        assert_eq!(parsed.head, "description");
        assert_eq!(parsed.args, vec!["\"Uplink to core\"", ";"]);
    }

    #[test]
    fn parse_junos_sets_named_dialect_hint() {
        let doc = parse_junos("set system host-name router-1\n");
        assert_eq!(
            doc.metadata.dialect_hint,
            DialectHint::Named("junos".into())
        );
    }
}
