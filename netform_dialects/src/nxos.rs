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

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::{Node, TriviaKind, parse_ios_like_parts};

    fn root_trivia(doc: &Document) -> Vec<(&str, TriviaKind)> {
        doc.roots
            .iter()
            .map(|id| match doc.node(*id).expect("node in arena") {
                Node::Line(line) => (line.raw.as_str(), line.trivia),
                Node::Block(block) => panic!("unexpected block {:?}", block.header.raw),
            })
            .collect()
    }

    #[test]
    fn nxos_hash_delimited_banner_body_is_literal() {
        let doc =
            parse("banner motd #\n! not a comment\ninterface Ethernet1/1\n#\nhostname spine-01\n");

        assert_eq!(
            root_trivia(&doc),
            vec![
                ("banner motd #", TriviaKind::Content),
                ("! not a comment", TriviaKind::Literal),
                ("interface Ethernet1/1", TriviaKind::Literal),
                ("#", TriviaKind::Literal),
                ("hostname spine-01", TriviaKind::Content),
            ],
        );
    }

    /// nxos-specific key hints; the arms nxos shares with eos and iosxe are in
    /// `crate::tests::ios_family_shared_key_hints`.
    #[test]
    fn key_hints() {
        for (line, expected) in [
            // interface types (0.5.0 table); ids are kept verbatim
            ("interface Ethernet1/1", Some("interface:ethernet:1/1")),
            ("interface Ethernet1/1/1", Some("interface:ethernet:1/1/1")),
            (
                "interface Ethernet1/1.100",
                Some("interface:ethernet:1/1.100"),
            ),
            ("interface ethernet1/1", Some("interface:ethernet:1/1")),
            ("interface ETHERNET1/1", Some("interface:ethernet:1/1")),
            (
                "interface port-channel10",
                Some("interface:port-channel:10"),
            ),
            (
                "interface Port-channel10",
                Some("interface:port-channel:10"),
            ),
            ("interface Vlan100", Some("interface:vlan:100")),
            ("interface vlan100", Some("interface:vlan:100")),
            ("interface loopback0", Some("interface:loopback:0")),
            ("interface Loopback0", Some("interface:loopback:0")),
            ("interface mgmt0", Some("interface:mgmt:0")),
            ("interface nve1", Some("interface:nve:1")),
            ("interface Nve1", Some("interface:nve:1")),
            ("interface tunnel0", Some("interface:tunnel:0")),
            ("interface Tunnel1", Some("interface:tunnel:1")),
            ("interface fabric1/1", Some("interface:fabric:1/1")),
            ("interface Fabric1/1", Some("interface:fabric:1/1")),
            // unknown types fall back to the raw name
            (
                "interface GigabitEthernet0/0/0",
                Some("interface:GigabitEthernet0/0/0"),
            ),
            ("vrf context MGMT", Some("vrf:MGMT")),
            ("feature ospf", Some("feature:ospf")),
            ("feature bgp", Some("feature:bgp")),
            ("feature vpc", Some("feature:vpc")),
            ("feature interface-vlan", Some("feature:interface-vlan")),
            ("vpc domain 10", Some("vpc-domain:10")),
            ("vpc domain 100", Some("vpc-domain:100")),
            ("vpc orphan-ports suspend", None),
            ("role name custom-admin", Some("role:custom-admin")),
            ("role feature-group name", None),
            ("system jumbomtu 9216", Some("system:jumbomtu")),
            ("system default switchport", Some("system:default")),
        ] {
            assert_eq!(
                key_hint(parse_ios_like_parts(line).as_ref()).as_deref(),
                expected,
                "{line}",
            );
        }
    }

    #[test]
    fn parse_round_trip() {
        let cfg = "\
hostname n9k-leaf-01
!
feature bgp
feature interface-vlan
feature lacp
feature vpc
!
vlan 10
  name SERVERS
vlan 20
  name MGMT
!
vpc domain 10
  peer-keepalive destination 10.1.1.2
!
interface Ethernet1/1
  description uplink-spine-a
  mtu 9216
  ip address 192.0.2.2/31
  no shutdown
interface port-channel10
  description vpc-peer-link
interface Vlan100
  ip address 10.10.1.1/24
interface loopback0
  ip address 10.255.255.1/32
interface mgmt0
  ip address 10.0.0.1/24
!
router bgp 65001
  router-id 10.255.255.1
";
        let doc = parse(cfg);
        assert_eq!(doc.render(), cfg);
    }

    #[test]
    fn parsed_document_carries_nxos_interface_hints() {
        let cfg = "interface Ethernet1/1\n  description uplink\n";
        let doc = parse(cfg);
        let first = match &doc.arena[doc.roots[0].0] {
            netform_ir::Node::Block(b) => &b.header,
            netform_ir::Node::Line(l) => l,
        };
        assert_eq!(first.key_hint.as_deref(), Some("interface:ethernet:1/1"),);
    }
}
