//! Cisco IOS XR-oriented dialect profile for `netform_ir`.
//!
//! this crate provides [`parse_iosxr`] and the reusable [`IOSXR_DIALECT`]
//! profile, which customize key-hint derivation for IOS XR-specific constructs
//! while reusing the shared IOS-like trivia classification and line
//! tokenization.  IOS XR blocks that close with a delimiter (`end-policy`,
//! `end-set`) keep it as their [`netform_ir::BlockNode`] footer.
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

use netform_ir::{
    Document, IosKeyHintConfig, IosLikeDialect, ParsedLineParts, common_key_hint,
    ios_family_key_hint, parse_with_dialect,
};

/// pre-built IOS XR dialect profile: IOS-like parsing with IOS XR-specific key
/// hints and delimiter-terminated policy and set blocks.
pub const IOSXR_DIALECT: IosLikeDialect =
    IosLikeDialect::new("iosxr", iosxr_key_hint).with_block_terminator(is_iosxr_block_terminator);

/// parse text using the IOS XR dialect ([`IOSXR_DIALECT`]).
pub fn parse_iosxr(input: &str) -> Document {
    parse_with_dialect(input, &IOSXR_DIALECT)
}

/// IOS XR interface type prefixes in canonical lowercase form.
///
/// longest-prefix-first (see `netform_ir::parse_interface`).
const IOSXR_INTERFACE_TYPES: &[&str] = &[
    "hundredgige",
    "bundle-ether",
    "gigabitethernet",
    "fortygige",
    "tunnel-ip",
    "pw-ether",
    "loopback",
    "tengige",
    "mgmteth",
    "bvi",
    "nve",
];

/// IOS XR-specific configuration for [`ios_family_key_hint`].
const IOSXR_KEY_HINT_CONFIG: IosKeyHintConfig = IosKeyHintConfig {
    interface_types: IOSXR_INTERFACE_TYPES,
    vrf_keyword: "vrf",
    extra_router_protos: &["isis", "eigrp"],
};

/// report whether `raw` closes an IOS XR block.
///
/// a bare `end` is not one: it closes the configuration session.
fn is_iosxr_block_terminator(raw: &str) -> bool {
    matches!(
        raw.trim(),
        "end-policy" | "end-set" | "end-class-map" | "end-policy-map" | "end-group"
    )
}

/// derive a stable identity key for IOS XR configuration lines.
///
/// delegates `interface`, `vrf`, `router`, and `ip` to
/// [`ios_family_key_hint`], handles IOS XR-specific constructs
/// (`route-policy`, the `*-set` families, the BGP `*-group` families), then
/// falls back to [`common_key_hint`] for the remaining shared arms.
fn iosxr_key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    if let Some(hint) = ios_family_key_hint(parsed, &IOSXR_KEY_HINT_CONFIG) {
        return Some(hint);
    }

    let parsed_ref = parsed?;
    let head = parsed_ref.head.as_str();
    let args = parsed_ref.args.as_slice();

    match head {
        "route-policy" => args.first().map(|name| format!("route-policy:{name}")),
        "prefix-set" => args.first().map(|name| format!("prefix-set:{name}")),
        "as-path-set" => args.first().map(|name| format!("as-path-set:{name}")),
        "community-set" => args.first().map(|name| format!("community-set:{name}")),
        "extcommunity-set" => match args {
            [kind, name, ..] => Some(format!("extcommunity-set:{kind}:{name}")),
            [name] => Some(format!("extcommunity-set:{name}")),
            _ => None,
        },
        "rd-set" => args.first().map(|name| format!("rd-set:{name}")),
        "neighbor-group" => args.first().map(|name| format!("neighbor-group:{name}")),
        "af-group" => args.first().map(|name| format!("af-group:{name}")),
        "session-group" => args.first().map(|name| format!("session-group:{name}")),
        "ipv4" => match args {
            [next, name, ..] if next == "access-list" => Some(format!("ipv4-access-list:{name}")),
            _ => None,
        },
        _ => common_key_hint(parsed),
    }
}
