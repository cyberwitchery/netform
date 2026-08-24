//! Arista EOS as registry data.

use crate::IosRules;
use crate::rules::{ArgGuard, KeyRule, KeyRuleAction};
use netform_ir::detect::{MODERATE_SIGNAL, NameShape, Signal, Test, WEAK_SIGNAL};
use netform_ir::{Document, IosLikeDialect, ParsedLineParts, parse_with_dialect};

/// EOS key-hint data.
pub const RULES: IosRules = IosRules {
    interface_types: &[
        "port-channel",
        "management",
        "ethernet",
        "loopback",
        "vxlan",
        "vlan",
    ],
    vrf_keyword: "instance",
    extra_router_protos: &["eigrp", "isis"],
    key_rules: &[
        KeyRule {
            head: "mlag",
            guards: &[ArgGuard::new(0, &["configuration"])],
            action: KeyRuleAction::literal("mlag"),
        },
        KeyRule {
            head: "management",
            guards: &[ArgGuard::new(0, &["api"])],
            action: KeyRuleAction::key("management-api", &[1]),
        },
        KeyRule {
            head: "management",
            guards: &[ArgGuard::new(0, &["ssh", "telnet", "console"])],
            action: KeyRuleAction::key("management", &[0, 1]),
        },
        KeyRule {
            head: "daemon",
            guards: &[],
            action: KeyRuleAction::key("daemon", &[0]),
        },
        KeyRule {
            head: "event-handler",
            guards: &[],
            action: KeyRuleAction::key("event-handler", &[0]),
        },
        KeyRule {
            head: "peer-filter",
            guards: &[],
            action: KeyRuleAction::key("peer-filter", &[0]),
        },
    ],
};

/// the EOS dialect profile.
pub const DIALECT: IosLikeDialect = IosLikeDialect::new("eos", key_hint);

/// derive a stable identity key for an EOS configuration line.
pub fn key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    RULES.key_hint(parsed)
}

/// parse text as EOS.
pub fn parse(input: &str) -> Document {
    parse_with_dialect(input, &DIALECT)
}

/// the patterns that make configuration text read as EOS: its non-slot
/// interface naming, CIDR addresses on interfaces, unqualified ACLs and their
/// sequence-numbered entries.
pub const SIGNALS: &[Signal] = &[
    Signal {
        weight: MODERATE_SIGNAL,
        tests: &[Test::InterfaceName(NameShape::StartsWithAny(&[
            "Management",
        ]))],
    },
    Signal {
        weight: MODERATE_SIGNAL,
        tests: &[
            Test::InterfaceName(NameShape::StartsWithAny(&["Ethernet"])),
            Test::Not(&Test::InterfaceName(NameShape::ContainsSlash)),
        ],
    },
    Signal {
        weight: MODERATE_SIGNAL,
        tests: &[
            Test::StartsWithAny(&["ip address "]),
            Test::WordContainsSlash(2),
        ],
    },
    Signal {
        weight: MODERATE_SIGNAL,
        tests: &[
            Test::StartsWithAny(&["ip access-list "]),
            Test::Not(&Test::ContainsAny(&["extended"])),
        ],
    },
    Signal {
        weight: WEAK_SIGNAL,
        tests: &[
            Test::WordIsNumber(0),
            Test::ContainsAny(&[" permit ", " deny "]),
        ],
    },
];

/// a canonical EOS excerpt.
pub const SAMPLE: &str = "\
interface Ethernet1
   description uplink-spine-1
   ip address 192.0.2.2/31
interface Management1
   ip address 10.0.0.5/24
mlag configuration
   domain-id leaf-pair-1
management api http-commands
   no shutdown
";
