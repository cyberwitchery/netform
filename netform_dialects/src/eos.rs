//! Arista EOS as registry data.

use crate::IosRules;
use crate::rules::{ArgGuard, KeyRule, KeyRuleAction};
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
