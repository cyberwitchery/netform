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

use netform_ir::{
    Dialect, DialectHint, Document, ParsedLineParts, TriviaKind, classify_ios_like_trivia,
    ios_like_key_hint, parse_ios_like_parts, parse_with_dialect,
};

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
        classify_ios_like_trivia(raw)
    }

    fn parse_parts(&self, raw: &str) -> Option<ParsedLineParts> {
        parse_ios_like_parts(raw)
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
        ios_like_key_hint(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iosxe_comment_classification_supports_bang_and_hash() {
        assert_eq!(classify_ios_like_trivia("!"), TriviaKind::Comment);
        assert_eq!(classify_ios_like_trivia("# generated"), TriviaKind::Comment);
        assert_eq!(
            classify_ios_like_trivia("interface Ethernet1"),
            TriviaKind::Content
        );
    }

    #[test]
    fn iosxe_tokenization_keeps_quoted_values_together() {
        let parsed =
            parse_ios_like_parts("description \"WAN uplink\"").expect("content should parse");
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
