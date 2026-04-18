//! Arista EOS-oriented dialect profile for `netform_ir`.
//!
//! This crate provides a conservative EOS profile that customizes:
//! - comment classification (`!`, `#`)
//! - tokenization with quoted-string preservation
//!
//! # Example
//!
//! ```rust
//! use netform_dialect_eos::parse_eos;
//!
//! let cfg = "interface Ethernet1\n   description \"Uplink\"\n";
//! let doc = parse_eos(cfg);
//! assert_eq!(doc.render(), cfg);
//! ```

use netform_ir::{
    Dialect, DialectHint, Document, ParsedLineParts, TriviaKind, classify_ios_like_trivia,
    ios_like_key_hint, parse_ios_like_parts, parse_with_dialect,
};

/// Dialect implementation for EOS-like configuration text.
#[derive(Debug, Default, Clone, Copy)]
pub struct EosDialect;

/// Parse text using [`EosDialect`].
pub fn parse_eos(input: &str) -> Document {
    parse_with_dialect(input, &EosDialect)
}

impl Dialect for EosDialect {
    fn dialect_hint(&self) -> DialectHint {
        DialectHint::Named("eos".to_string())
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
    fn eos_comment_classification_supports_bang_and_hash() {
        assert_eq!(classify_ios_like_trivia("!"), TriviaKind::Comment);
        assert_eq!(classify_ios_like_trivia("# generated"), TriviaKind::Comment);
        assert_eq!(classify_ios_like_trivia("vlan 10"), TriviaKind::Content);
    }

    #[test]
    fn eos_tokenization_keeps_quoted_values_together() {
        let parsed =
            parse_ios_like_parts("description \"Transit uplink\"").expect("content should parse");
        assert_eq!(parsed.head, "description");
        assert_eq!(parsed.args, vec!["\"Transit uplink\""]);
    }

    #[test]
    fn parse_eos_sets_named_dialect_hint() {
        let doc = parse_eos("hostname leaf-01\n");
        assert_eq!(doc.metadata.dialect_hint, DialectHint::Named("eos".into()));
    }
}
