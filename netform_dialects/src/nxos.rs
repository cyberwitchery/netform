//! Cisco NX-OS as registry data.

use crate::IosRules;
use crate::rules::{ArgGuard, KeyRule, KeyRuleAction};
use netform_ir::detect::{MODERATE_SIGNAL, NameShape, STRONG_SIGNAL, Signal, Test};
use netform_ir::{Document, IosLikeDialect, ParsedLineParts, parse_with_dialect};

/// NX-OS key-hint data.
pub const RULES: IosRules = IosRules {
    interface_types: &[
        "port-channel",
        "ethernet",
        "loopback",
        "fabric",
        "tunnel",
        "vlan",
        "mgmt",
        "nve",
    ],
    vrf_keyword: "context",
    extra_router_protos: &["eigrp", "isis"],
    key_rules: &[
        KeyRule {
            head: "feature",
            guards: &[],
            action: KeyRuleAction::key("feature", &[0]),
        },
        KeyRule {
            head: "vpc",
            guards: &[ArgGuard::new(0, &["domain"])],
            action: KeyRuleAction::key("vpc-domain", &[1]),
        },
        KeyRule {
            head: "role",
            guards: &[ArgGuard::new(0, &["name"])],
            action: KeyRuleAction::key("role", &[1]),
        },
        KeyRule {
            head: "system",
            guards: &[],
            action: KeyRuleAction::key("system", &[0]),
        },
    ],
};

/// the NX-OS dialect profile.
pub const DIALECT: IosLikeDialect = IosLikeDialect::new("nxos", key_hint);

/// derive a stable identity key for an NX-OS configuration line.
pub fn key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    RULES.key_hint(parsed)
}

/// parse text as NX-OS.
pub fn parse(input: &str) -> Document {
    parse_with_dialect(input, &DIALECT)
}

/// the patterns that make configuration text read as NX-OS: `feature`, vPC,
/// RBAC roles and its plain `Ethernet<slot>/<port>` interface naming.
pub const SIGNALS: &[Signal] = &[
    Signal {
        weight: STRONG_SIGNAL,
        tests: &[Test::StartsWithAny(&["feature "])],
    },
    Signal {
        weight: STRONG_SIGNAL,
        tests: &[
            Test::InterfaceName(NameShape::StartsWithAny(&["Ethernet"])),
            Test::InterfaceName(NameShape::ContainsSlash),
        ],
    },
    Signal {
        weight: STRONG_SIGNAL,
        tests: &[Test::StartsWithAny(&["vpc "])],
    },
    Signal {
        weight: MODERATE_SIGNAL,
        tests: &[Test::StartsWithAny(&["role name "])],
    },
];

/// a canonical NX-OS excerpt.
pub const SAMPLE: &str = "\
feature bgp
feature interface-vlan
feature vpc
vpc domain 10
  peer-switch
interface Ethernet1/1
  description leaf-uplink
  switchport mode trunk
interface port-channel10
  vpc peer-link
";
