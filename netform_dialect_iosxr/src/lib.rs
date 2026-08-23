//! Cisco IOS XR-oriented dialect profile for `netform_ir`.
//!
//! this crate is the published face of the `iosxr` entry in
//! [`netform_dialects::REGISTRY`]; the interface-type table, VRF keyword and
//! key-hint rules it exposes live there as data.  IOS XR blocks that close
//! with a delimiter (`end-policy`, `end-set`) keep it as their
//! [`netform_ir::BlockNode`] footer.
//!
//! # Example
//!
//! ```rust
//! use netform_dialect_iosxr::parse_iosxr;
//!
//! let cfg = "route-policy PASS-ALL\n  pass\nend-policy\n";
//! let doc = parse_iosxr(cfg);
//! assert_eq!(doc.render(), cfg);
//! ```

use netform_ir::{Document, IosLikeDialect};

/// pre-built Cisco IOS XR dialect profile.
pub const IOSXR_DIALECT: IosLikeDialect = netform_dialects::iosxr::DIALECT;

/// parse text using the Cisco IOS XR dialect ([`IOSXR_DIALECT`]).
pub fn parse_iosxr(input: &str) -> Document {
    netform_dialects::iosxr::parse(input)
}
