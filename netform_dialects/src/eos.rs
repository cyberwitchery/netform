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
    route_distance_is_identity: true,
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

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::{Node, TriviaKind, classify_ios_like_trivia, parse_ios_like_parts};

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
    fn eos_delimiter_less_banner_body_ends_at_eof() {
        let doc = parse("banner motd\n! not a comment\nEOF\nhostname leaf-01\n");

        assert_eq!(
            root_trivia(&doc),
            vec![
                ("banner motd", TriviaKind::Content),
                ("! not a comment", TriviaKind::Literal),
                ("EOF", TriviaKind::Literal),
                ("hostname leaf-01", TriviaKind::Content),
            ],
        );
    }

    #[test]
    fn eos_delimiter_banner_body_ends_at_the_delimiter() {
        let doc = parse("banner login ^C\nRestricted\n^C\nhostname leaf-01\n");

        assert_eq!(
            root_trivia(&doc),
            vec![
                ("banner login ^C", TriviaKind::Content),
                ("Restricted", TriviaKind::Literal),
                ("^C", TriviaKind::Literal),
                ("hostname leaf-01", TriviaKind::Content),
            ],
        );
    }

    /// the IOS-family trivia classifier eos, iosxe and nxos share.
    #[test]
    fn ios_like_comment_classification_supports_bang_and_hash() {
        for (raw, expected) in [
            ("!", TriviaKind::Comment),
            ("# generated", TriviaKind::Comment),
            ("interface Ethernet1", TriviaKind::Content),
            ("interface Ethernet1/1", TriviaKind::Content),
            ("interface GigabitEthernet0/0/0", TriviaKind::Content),
        ] {
            assert_eq!(classify_ios_like_trivia(raw), expected, "{raw}");
        }
    }

    /// the IOS-family tokenizer eos, iosxe and nxos share.
    #[test]
    fn ios_like_tokenization_keeps_quoted_values_together() {
        for value in [
            "\"Transit uplink\"",
            "\"WAN uplink\"",
            "\"Uplink to spine\"",
        ] {
            let line = format!("description {value}");
            let parsed = parse_ios_like_parts(&line).expect("content should parse");
            assert_eq!(parsed.head, "description", "{line}");
            assert_eq!(parsed.args, vec![value], "{line}");
        }
    }

    /// eos-specific key hints; the arms eos shares with iosxe and nxos are in
    /// `crate::tests::ios_family_shared_key_hints`.
    #[test]
    fn key_hints() {
        for (line, expected) in [
            // interface types (0.6.0 table); ids are kept verbatim
            ("interface Ethernet1", Some("interface:ethernet:1")),
            ("interface Ethernet1/1", Some("interface:ethernet:1/1")),
            ("interface Ethernet1.100", Some("interface:ethernet:1.100")),
            ("interface ethernet1", Some("interface:ethernet:1")),
            ("interface ETHERNET1", Some("interface:ethernet:1")),
            (
                "interface Port-Channel10",
                Some("interface:port-channel:10"),
            ),
            (
                "interface port-channel10",
                Some("interface:port-channel:10"),
            ),
            ("interface Vlan100", Some("interface:vlan:100")),
            ("interface vlan100", Some("interface:vlan:100")),
            ("interface Loopback0", Some("interface:loopback:0")),
            ("interface loopback0", Some("interface:loopback:0")),
            ("interface Management1", Some("interface:management:1")),
            ("interface management1", Some("interface:management:1")),
            ("interface Vxlan1", Some("interface:vxlan:1")),
            ("interface vxlan1", Some("interface:vxlan:1")),
            // unknown types fall back to the raw name
            (
                "interface GigabitEthernet0/0/0",
                Some("interface:GigabitEthernet0/0/0"),
            ),
            ("vrf instance MGMT", Some("vrf:MGMT")),
            ("mlag configuration", Some("mlag")),
            ("mlag", None),
            (
                "management api http-commands",
                Some("management-api:http-commands"),
            ),
            ("management api gnmi", Some("management-api:gnmi")),
            ("management api restful", Some("management-api:restful")),
            (
                "management ssh idle-timeout 15",
                Some("management:ssh:idle-timeout"),
            ),
            ("daemon TerminAttr", Some("daemon:TerminAttr")),
            ("daemon myagent", Some("daemon:myagent")),
            (
                "event-handler lnterface-recovery",
                Some("event-handler:lnterface-recovery"),
            ),
            ("peer-filter LEAF-PEERS", Some("peer-filter:LEAF-PEERS")),
            // a static route's distance is part of its identity on eos
            (
                "ip route 4.4.4.4/32 Ethernet1 7",
                Some("ip-route:4.4.4.4/32:Ethernet1:7"),
            ),
            (
                "ip route vrf MGMT 10.0.0.0 255.0.0.0 192.168.1.1 200 name X",
                Some("ip-route:MGMT:10.0.0.0/8:192.168.1.1:200"),
            ),
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
hostname leaf-01
!
vlan 10
  name SERVERS
vlan 20
  name MGMT
!
vrf instance MGMT
  rd 10.255.255.1:1000
!
mlag configuration
  domain-id MLAG-DOMAIN
  local-interface Vlan4094
  peer-link Port-Channel1
!
interface Ethernet1
  description uplink-spine-a
  mtu 9214
  ip address 192.0.2.2/31
  no shutdown
interface Port-Channel10
  description mlag-peer-link
interface Vlan100
  ip address 10.10.1.1/24
interface Loopback0
  ip address 10.255.255.1/32
interface Management1
  ip address 10.0.0.1/24
interface Vxlan1
  vxlan source-interface Loopback1
!
management api http-commands
  no shutdown
!
daemon TerminAttr
  exec /usr/bin/TerminAttr
!
router bgp 65000
  router-id 10.255.255.1
";
        let doc = parse(cfg);
        assert_eq!(doc.render(), cfg);
    }

    #[test]
    fn parsed_document_carries_eos_interface_hints() {
        let cfg = "interface Ethernet1\n  description uplink\n";
        let doc = parse(cfg);
        let first = match &doc.arena[doc.roots[0].0] {
            netform_ir::Node::Block(b) => &b.header,
            netform_ir::Node::Line(l) => l,
        };
        assert_eq!(first.key_hint.as_deref(), Some("interface:ethernet:1"));
    }
}
