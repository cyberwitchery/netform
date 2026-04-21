//! Arista EOS-oriented dialect profile for `netform_ir`.
//!
//! This crate re-exports [`IosLikeDialect`] parameterised to `"eos"` and
//! provides the [`parse_eos`] convenience function.
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

use netform_ir::{Document, IosLikeDialect, parse_with_dialect};

/// Pre-built EOS dialect instance.
pub const EOS_DIALECT: IosLikeDialect = IosLikeDialect::new("eos");

/// Backward-compatible type alias for the EOS dialect.
pub type EosDialect = IosLikeDialect;

/// Parse text using the EOS dialect.
pub fn parse_eos(input: &str) -> Document {
    parse_with_dialect(input, &EOS_DIALECT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::{DialectHint, TriviaKind, classify_ios_like_trivia, parse_ios_like_parts};

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
