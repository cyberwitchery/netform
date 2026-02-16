//! IOS XE-oriented dialect profile for `netform_ir`.
//!
//! This crate provides a conservative IOS XE profile that customizes:
//! - comment classification (`!`, `#`)
//! - tokenization with quoted-string preservation
//!
//! # Example
//!
//! ```rust
//! use netform_dialect_iosxe::parse_iosxe;
//!
//! let cfg = "interface Ethernet1\n  description \"WAN uplink\"\n";
//! let doc = parse_iosxe(cfg);
//! assert_eq!(doc.render(), cfg);
//! ```

use netform_ir::{Dialect, DialectHint, Document, ParsedLineParts, TriviaKind, parse_with_dialect};

/// Dialect implementation for IOS XE-like configuration text.
#[derive(Debug, Default, Clone, Copy)]
pub struct IosxeDialect;

/// Parse text using [`IosxeDialect`].
pub fn parse_iosxe(input: &str) -> Document {
    parse_with_dialect(input, &IosxeDialect)
}

impl Dialect for IosxeDialect {
    fn dialect_hint(&self) -> DialectHint {
        DialectHint::Named("iosxe".to_string())
    }

    fn classify_trivia(&self, raw: &str) -> TriviaKind {
        classify_iosxe_trivia(raw)
    }

    fn parse_parts(&self, raw: &str) -> Option<ParsedLineParts> {
        parse_iosxe_parts(raw)
    }
}

fn classify_iosxe_trivia(raw: &str) -> TriviaKind {
    if raw.trim().is_empty() {
        return TriviaKind::Blank;
    }

    let trimmed = raw.trim_start();
    if trimmed.starts_with('!') || trimmed.starts_with('#') {
        return TriviaKind::Comment;
    }

    TriviaKind::Content
}

fn parse_iosxe_parts(raw: &str) -> Option<ParsedLineParts> {
    let tokens = tokenize_iosxe(raw);
    let head = tokens.first()?.clone();
    let args = tokens.into_iter().skip(1).collect::<Vec<_>>();
    Some(ParsedLineParts { head, args })
}

fn tokenize_iosxe(raw: &str) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iosxe_comment_classification_supports_bang_and_hash() {
        assert_eq!(classify_iosxe_trivia("!"), TriviaKind::Comment);
        assert_eq!(classify_iosxe_trivia("# generated"), TriviaKind::Comment);
        assert_eq!(
            classify_iosxe_trivia("interface Ethernet1"),
            TriviaKind::Content
        );
    }

    #[test]
    fn iosxe_tokenization_keeps_quoted_values_together() {
        let parsed = parse_iosxe_parts("description \"WAN uplink\"").expect("content should parse");
        assert_eq!(parsed.head, "description");
        assert_eq!(parsed.args, vec!["\"WAN uplink\""]);
    }

    #[test]
    fn parse_iosxe_sets_named_dialect_hint() {
        let doc = parse_iosxe("hostname edge-1\n");
        assert_eq!(
            doc.metadata.dialect_hint,
            DialectHint::Named("iosxe".into())
        );
    }
}
