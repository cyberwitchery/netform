//! Cisco IOS XE as registry data.

use crate::IosRules;
use crate::rules::{ArgGuard, KeyRule, KeyRuleAction};
use netform_ir::detect::{MODERATE_SIGNAL, NameShape, STRONG_SIGNAL, Signal, Test, WEAK_SIGNAL};
use netform_ir::{Document, IosLikeDialect, ParsedLineParts, parse_with_dialect};

/// IOS XE key-hint data.
pub const RULES: IosRules = IosRules {
    interface_types: &[
        "appgigabitethernet",
        "fortygigabitethernet",
        "fivegigabitethernet",
        "twogigabitethernet",
        "tengigabitethernet",
        "twentyfivegige",
        "gigabitethernet",
        "fastethernet",
        "hundredgige",
        "port-channel",
        "loopback",
        "tunnel",
        "serial",
        "vlan",
        "bdi",
    ],
    vrf_keyword: "definition",
    extra_router_protos: &["eigrp", "isis"],
    key_rules: &[
        KeyRule {
            head: "crypto",
            guards: &[
                ArgGuard::new(0, &["pki"]),
                ArgGuard::new(1, &["certificate"]),
                ArgGuard::new(2, &["chain"]),
            ],
            action: KeyRuleAction::key("crypto:pki:certificate-chain", &[3]),
        },
        KeyRule {
            head: "crypto",
            guards: &[ArgGuard::new(0, &["pki"])],
            action: KeyRuleAction::key("crypto:pki", &[1, 2]),
        },
        KeyRule {
            head: "crypto",
            guards: &[],
            action: KeyRuleAction::Common,
        },
        KeyRule {
            head: "redundancy",
            guards: &[],
            action: KeyRuleAction::literal("redundancy"),
        },
        KeyRule {
            head: "parameter-map",
            guards: &[ArgGuard::new(0, &["type"])],
            action: KeyRuleAction::key("parameter-map", &[1, 2]),
        },
        KeyRule {
            head: "track",
            guards: &[],
            action: KeyRuleAction::key("track", &[0]),
        },
        KeyRule {
            head: "zone",
            guards: &[ArgGuard::new(0, &["security"])],
            action: KeyRuleAction::key("zone-security", &[1]),
        },
        KeyRule {
            head: "zone-pair",
            guards: &[ArgGuard::new(0, &["security"])],
            action: KeyRuleAction::key("zone-pair", &[1]),
        },
    ],
};

/// the IOS XE dialect profile.
pub const DIALECT: IosLikeDialect = IosLikeDialect::new("iosxe", key_hint);

/// derive a stable identity key for an IOS XE configuration line.
pub fn key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    RULES.key_hint(parsed)
}

/// parse text as IOS XE.
pub fn parse(input: &str) -> Document {
    parse_with_dialect(input, &DIALECT)
}

/// the patterns that make configuration text read as IOS XE: its
/// speed-prefixed Ethernet naming, extended ACLs, and the dotted-decimal masks
/// it writes where the other vendors write prefix lengths.
pub const SIGNALS: &[Signal] = &[
    Signal {
        weight: MODERATE_SIGNAL,
        tests: &[Test::InterfaceName(NameShape::IosxeEthernet)],
    },
    Signal {
        weight: STRONG_SIGNAL,
        tests: &[Test::StartsWithAny(&["ip access-list extended "])],
    },
    Signal {
        weight: MODERATE_SIGNAL,
        tests: &[
            Test::StartsWithAny(&["ip address "]),
            Test::WordIsDottedMask(3),
        ],
    },
    Signal {
        weight: MODERATE_SIGNAL,
        tests: &[
            Test::StartsWithAny(&["network "]),
            Test::ContainsAny(&[" mask "]),
        ],
    },
    Signal {
        weight: WEAK_SIGNAL,
        tests: &[
            Test::StartsWithAny(&["permit ", "deny "]),
            Test::AnyWordIsDottedMask,
        ],
    },
];

/// a canonical IOS XE excerpt.
pub const SAMPLE: &str = "\
interface GigabitEthernet0/0/0
  description uplink-core-a
  ip address 192.0.2.2 255.255.255.252
interface TenGigabitEthernet0/1/0
  switchport mode trunk
ip access-list extended MGMT-IN
  permit tcp any any eq 22
  deny   ip 10.0.0.0 0.255.255.255 any
";
