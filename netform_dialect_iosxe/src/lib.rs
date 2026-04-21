//! IOS XE-oriented dialect profile for `netform_ir`.
//!
//! This crate re-exports [`IosLikeDialect`] parameterised to `"iosxe"` and
//! provides the [`parse_iosxe`] convenience function.
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

use netform_ir::{Document, IosLikeDialect, parse_with_dialect};

/// Pre-built IOS XE dialect instance.
pub const IOSXE_DIALECT: IosLikeDialect = IosLikeDialect::new("iosxe");

/// Backward-compatible type alias for the IOS XE dialect.
pub type IosxeDialect = IosLikeDialect;

/// Parse text using the IOS XE dialect.
pub fn parse_iosxe(input: &str) -> Document {
    parse_with_dialect(input, &IOSXE_DIALECT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::{DialectHint, TriviaKind, classify_ios_like_trivia, parse_ios_like_parts};

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
