//! Cisco IOS XR as registry data.

use crate::IosRules;
use crate::rules::{ArgGuard, KeyRule, KeyRuleAction};
use netform_ir::detect::{MODERATE_SIGNAL, NameShape, STRONG_SIGNAL, Signal, Test};
use netform_ir::{Document, IosLikeDialect, ParsedLineParts, parse_with_dialect};

/// IOS XR key-hint data.
pub const RULES: IosRules = IosRules {
    interface_types: &[
        "gigabitethernet",
        "bundle-ether",
        "hundredgige",
        "fortygige",
        "tunnel-ip",
        "loopback",
        "pw-ether",
        "mgmteth",
        "tengige",
        "bvi",
        "nve",
    ],
    vrf_keyword: "vrf",
    extra_router_protos: &["isis"],
    key_rules: &[
        KeyRule {
            head: "route-policy",
            guards: &[],
            action: KeyRuleAction::key("route-policy", &[0]),
        },
        KeyRule {
            head: "prefix-set",
            guards: &[],
            action: KeyRuleAction::key("prefix-set", &[0]),
        },
        KeyRule {
            head: "as-path-set",
            guards: &[],
            action: KeyRuleAction::key("as-path-set", &[0]),
        },
        KeyRule {
            head: "community-set",
            guards: &[],
            action: KeyRuleAction::key("community-set", &[0]),
        },
        KeyRule {
            head: "extcommunity-set",
            guards: &[],
            action: KeyRuleAction::key("extcommunity-set", &[0, 1]),
        },
        KeyRule {
            head: "extcommunity-set",
            guards: &[],
            action: KeyRuleAction::key("extcommunity-set", &[0]),
        },
        KeyRule {
            head: "rd-set",
            guards: &[],
            action: KeyRuleAction::key("rd-set", &[0]),
        },
        KeyRule {
            head: "neighbor-group",
            guards: &[],
            action: KeyRuleAction::key("neighbor-group", &[0]),
        },
        KeyRule {
            head: "af-group",
            guards: &[],
            action: KeyRuleAction::key("af-group", &[0]),
        },
        KeyRule {
            head: "session-group",
            guards: &[],
            action: KeyRuleAction::key("session-group", &[0]),
        },
        KeyRule {
            head: "ipv4",
            guards: &[ArgGuard::new(0, &["access-list"])],
            action: KeyRuleAction::key("ipv4-access-list", &[1]),
        },
    ],
};

/// the IOS XR dialect profile, including its delimiter-terminated policy and
/// set blocks.
pub const DIALECT: IosLikeDialect =
    IosLikeDialect::new("iosxr", key_hint).with_block_terminator(is_block_terminator);

/// report whether `raw` closes an IOS XR block.
///
/// a bare `end` is not one: it closes the configuration session.
fn is_block_terminator(raw: &str) -> bool {
    matches!(
        raw.trim(),
        "end-policy" | "end-set" | "end-class-map" | "end-policy-map" | "end-group"
    )
}

/// derive a stable identity key for an IOS XR configuration line.
pub fn key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    RULES.key_hint(parsed)
}

/// parse text as IOS XR.
pub fn parse(input: &str) -> Document {
    parse_with_dialect(input, &DIALECT)
}

/// the patterns that make configuration text read as IOS XR: Routing Policy
/// Language and its `*-set` families, the BGP group templates, `ipv4`-scoped
/// addressing, and the interface names only XR spells.
pub const SIGNALS: &[Signal] = &[
    Signal {
        weight: STRONG_SIGNAL,
        tests: &[Test::InterfaceName(NameShape::Iosxr)],
    },
    Signal {
        weight: STRONG_SIGNAL,
        tests: &[Test::StartsWithAny(&[
            "route-policy ",
            "prefix-set ",
            "as-path-set ",
            "community-set ",
            "extcommunity-set ",
            "rd-set ",
        ])],
    },
    Signal {
        weight: MODERATE_SIGNAL,
        tests: &[Test::IsAny(&["end-policy", "end-set"])],
    },
    Signal {
        weight: STRONG_SIGNAL,
        tests: &[Test::StartsWithAny(&[
            "neighbor-group ",
            "af-group ",
            "session-group ",
        ])],
    },
    Signal {
        weight: MODERATE_SIGNAL,
        tests: &[Test::StartsWithAny(&["ipv4 address ", "ipv4 access-list "])],
    },
];

/// a canonical IOS XR excerpt.
pub const SAMPLE: &str = "\
interface HundredGigE0/0/0/0
 description spine-uplink-1
 ipv4 address 192.0.2.2 255.255.255.252
interface Bundle-Ether10
 description core-bundle
route-policy PASS-ALL
  pass
end-policy
prefix-set CUSTOMER-V4
  198.51.100.0/24
end-set
";
