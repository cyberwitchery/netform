//! Huawei VRP-oriented dialect profile for `netform_ir`.
//!
//! this crate provides [`parse_vrp`] and the reusable [`VRP_DIALECT`] profile,
//! which customize key-hint derivation for VRP-specific constructs while
//! reusing the shared IOS-like trivia classification and line tokenization.
//! VRP's `#` section separators are read as comments by that shared
//! classification.
//!
//! # Example
//!
//! ```rust
//! use netform_dialect_vrp::parse_vrp;
//!
//! let cfg = "#\ninterface Vlanif10\n ip address 10.0.10.1 255.255.255.0\n#\n";
//! let doc = parse_vrp(cfg);
//! assert_eq!(doc.render(), cfg);
//! ```

use netform_ir::{
    Document, IosKeyHintConfig, IosLikeDialect, ParsedLineParts, common_key_hint,
    ios_family_key_hint, parse_with_dialect, vrp_literal_region,
};

/// pre-built Huawei VRP dialect profile: IOS-like parsing with VRP-specific
/// key hints.
pub const VRP_DIALECT: IosLikeDialect =
    IosLikeDialect::new("vrp", vrp_key_hint).with_literal_region(vrp_literal_region);

/// parse text using the Huawei VRP dialect ([`VRP_DIALECT`]).
pub fn parse_vrp(input: &str) -> Document {
    parse_with_dialect(input, &VRP_DIALECT)
}

/// Huawei VRP interface type prefixes in canonical lowercase form.
///
/// longest-prefix-first (see `netform_ir::parse_interface`).
///
/// public so `netform_cli`'s `detect_guard_coverage` suite can assert that no
/// entry here is read as IOS XR at the slot depths VRP writes.
pub const VRP_INTERFACE_TYPES: &[&str] = &[
    "virtual-template",
    "xgigabitethernet",
    "gigabitethernet",
    "eth-trunk",
    "ip-trunk",
    "loopback",
    "vlanif",
    "tunnel",
    "serial",
    "100ge",
    "null",
    "meth",
    "25ge",
    "40ge",
    "pos",
];

/// VRP-specific configuration for [`ios_family_key_hint`].
///
/// `vrf_keyword` and `extra_router_protos` are inert: VRP spells its VRFs
/// `ip vpn-instance` and enters `bgp`/`ospf`/`isis` with no `router` keyword,
/// so `vrp_key_hint` keys both itself.
const VRP_KEY_HINT_CONFIG: IosKeyHintConfig = IosKeyHintConfig {
    interface_types: VRP_INTERFACE_TYPES,
    vrf_keyword: "vpn-instance",
    extra_router_protos: &[],
};

/// derive a stable identity key for Huawei VRP configuration lines.
///
/// delegates `interface` to [`ios_family_key_hint`], handles VRP-specific
/// constructs (`ip vpn-instance`, `acl`, the `traffic` families,
/// `route-policy`, `user-interface`, `local-user`, `peer`, and the bare
/// `bgp`/`ospf`/`isis` router views), then falls back to [`common_key_hint`]
/// for the remaining shared arms.
fn vrp_key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    let parsed_ref = parsed?;
    let head = parsed_ref.head.as_str();
    let args = parsed_ref.args.as_slice();

    if head == "ip"
        && let Some(hint) = vrp_ip_key_hint(args)
    {
        return Some(hint);
    }

    if let Some(hint) = ios_family_key_hint(parsed, &VRP_KEY_HINT_CONFIG) {
        return Some(hint);
    }

    match head {
        // `vlan batch 10 20` declares a set, not one VLAN; its identity is its text.
        "vlan" if args.first().is_some_and(|first| first == "batch") => None,
        "acl" => match args {
            [kind, name, ..] if kind == "number" || kind == "name" => Some(format!("acl:{name}")),
            [number, ..] => Some(format!("acl:{number}")),
            _ => None,
        },
        "traffic" => match args {
            [kind, name, ..] if kind == "classifier" || kind == "behavior" || kind == "policy" => {
                Some(format!("traffic-{kind}:{name}"))
            }
            _ => None,
        },
        "route-policy" => match args {
            [name, action, node_kw, seq, ..] if node_kw == "node" => {
                Some(format!("route-policy:{name}:{action}:{seq}"))
            }
            [name, ..] => Some(format!("route-policy:{name}")),
            _ => None,
        },
        "user-interface" => match args {
            // keyword-plus-value, not type-plus-range: the count is not the identity.
            [kind, ..] if kind == "maximum-vty" => Some("user-interface:maximum-vty".into()),
            [kind, from, to, ..] => Some(format!("user-interface:{kind}:{from}:{to}")),
            [kind, one] => Some(format!("user-interface:{kind}:{one}")),
            [kind] => Some(format!("user-interface:{kind}")),
            _ => None,
        },
        "local-user" => match args {
            [name, attr, ..] => Some(format!("local-user:{name}:{attr}")),
            [name] => Some(format!("local-user:{name}")),
            _ => None,
        },
        "peer" => match args {
            [addr, attr, ..] => Some(format!("peer:{addr}:{attr}")),
            [addr] => Some(format!("peer:{addr}")),
            _ => None,
        },
        "bgp" | "ospf" | "isis" => match args {
            [id, ..] if is_process_id(id) => Some(format!("router:{head}:{id}")),
            _ => None,
        },
        _ => common_key_hint(parsed),
    }
}

/// derive a key hint for the `ip` constructs VRP spells differently from the
/// IOS family.  returns `None` for the rest, which [`ios_family_key_hint`]
/// still sees.
fn vrp_ip_key_hint(args: &[String]) -> Option<String> {
    match args {
        [next, name, ..] if next == "vpn-instance" => Some(format!("vpn-instance:{name}")),
        [next, name, idx_kw, index, ..] if next == "ip-prefix" && idx_kw == "index" => {
            Some(format!("ip-prefix:{name}:{index}"))
        }
        [next, name, ..] if next == "ip-prefix" => Some(format!("ip-prefix:{name}")),
        [next, vrf_kw, vrf_name, dest, mask, ..]
            if next == "route-static" && vrf_kw == "vpn-instance" =>
        {
            Some(format!("ip-route-static:{vrf_name}:{dest}:{mask}"))
        }
        [next, dest, mask, ..] if next == "route-static" => {
            Some(format!("ip-route-static:{dest}:{mask}"))
        }
        _ => None,
    }
}

/// report whether `arg` is a VRP process or AS identifier (`1`, `65000`,
/// `65000.1`) rather than a sub-command keyword.
fn is_process_id(arg: &str) -> bool {
    arg.starts_with(|c: char| c.is_ascii_digit())
        && arg.chars().all(|c| c.is_ascii_digit() || c == '.')
}
