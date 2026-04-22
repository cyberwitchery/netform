//! Cisco NX-OS-oriented dialect profile for `netform_ir`.
//!
//! This crate re-exports [`IosLikeDialect`] parameterised to `"nxos"` and
//! provides the [`parse_nxos`] convenience function.
//!
//! # Example
//!
//! ```rust
//! use netform_dialect_nxos::parse_nxos;
//!
//! let cfg = "interface Ethernet1/1\n  description Uplink\n";
//! let doc = parse_nxos(cfg);
//! assert_eq!(doc.render(), cfg);
//! ```

use netform_ir::{Document, IosLikeDialect, parse_with_dialect};

/// Pre-built NX-OS dialect instance.
pub const NXOS_DIALECT: IosLikeDialect = IosLikeDialect::new("nxos");

/// Backward-compatible type alias for the NX-OS dialect.
pub type NxosDialect = IosLikeDialect;

/// Parse text using the NX-OS dialect.
pub fn parse_nxos(input: &str) -> Document {
    parse_with_dialect(input, &NXOS_DIALECT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::{DialectHint, TriviaKind, classify_ios_like_trivia, parse_ios_like_parts};

    #[test]
    fn nxos_comment_classification_supports_bang_and_hash() {
        assert_eq!(classify_ios_like_trivia("!"), TriviaKind::Comment);
        assert_eq!(classify_ios_like_trivia("# generated"), TriviaKind::Comment);
        assert_eq!(
            classify_ios_like_trivia("interface Ethernet1/1"),
            TriviaKind::Content
        );
    }

    #[test]
    fn nxos_tokenization_keeps_quoted_values_together() {
        let parsed =
            parse_ios_like_parts("description \"Uplink to spine\"").expect("content should parse");
        assert_eq!(parsed.head, "description");
        assert_eq!(parsed.args, vec!["\"Uplink to spine\""]);
    }

    #[test]
    fn parse_nxos_sets_named_dialect_hint() {
        let doc = parse_nxos("hostname n9k-leaf-01\n");
        assert_eq!(doc.metadata.dialect_hint, DialectHint::Named("nxos".into()));
    }
}
