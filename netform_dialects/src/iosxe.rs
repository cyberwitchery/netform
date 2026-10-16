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

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::{Node, TriviaKind, parse_ios_like_parts};

    /// `(raw, trivia, key_hint)` for each root line of a flat document.
    fn root_lines(doc: &Document) -> Vec<(&str, TriviaKind, Option<&str>)> {
        doc.roots
            .iter()
            .map(|id| match doc.node(*id).expect("node in arena") {
                Node::Line(line) => (line.raw.as_str(), line.trivia, line.key_hint.as_deref()),
                Node::Block(block) => panic!("unexpected block {:?}", block.header.raw),
            })
            .collect()
    }

    #[test]
    fn iosxe_banner_body_is_opaque_literal_text() {
        let doc = parse(
            "banner motd ^C\n! not a comment\ninterface GigabitEthernet0/0/0\n^C\nhostname edge-1\n",
        );

        assert_eq!(
            root_lines(&doc),
            vec![
                ("banner motd ^C", TriviaKind::Content, None),
                ("! not a comment", TriviaKind::Literal, None),
                ("interface GigabitEthernet0/0/0", TriviaKind::Literal, None),
                ("^C", TriviaKind::Literal, None),
                ("hostname edge-1", TriviaKind::Content, None),
            ],
        );
    }

    #[test]
    fn iosxe_interface_after_a_banner_still_gets_its_key_hint() {
        let doc = parse(
            "banner motd ^C\ninterface GigabitEthernet0/0/0\n^C\ninterface GigabitEthernet0/0/0\n",
        );

        let hints: Vec<_> = root_lines(&doc)
            .into_iter()
            .filter_map(|(_, _, hint)| hint.map(str::to_string))
            .collect();
        assert_eq!(hints, vec!["interface:gigabitethernet:0/0/0"]);
    }

    fn hint(line: &str) -> Option<String> {
        let parsed = parse_ios_like_parts(line);
        key_hint(parsed.as_ref())
    }

    /// iosxe-specific key hints; the arms iosxe shares with eos and nxos are in
    /// `crate::tests::ios_family_shared_key_hints`.
    #[test]
    fn key_hints() {
        for (line, expected) in [
            // interface types (0.6.0 table); ids are kept verbatim
            (
                "interface GigabitEthernet0/0/0",
                Some("interface:gigabitethernet:0/0/0"),
            ),
            (
                "interface GigabitEthernet1/0/1",
                Some("interface:gigabitethernet:1/0/1"),
            ),
            (
                "interface GigabitEthernet0/0/0.100",
                Some("interface:gigabitethernet:0/0/0.100"),
            ),
            (
                "interface gigabitethernet0/0/0",
                Some("interface:gigabitethernet:0/0/0"),
            ),
            (
                "interface GIGABITETHERNET0/0/0",
                Some("interface:gigabitethernet:0/0/0"),
            ),
            (
                "interface TenGigabitEthernet1/0/1",
                Some("interface:tengigabitethernet:1/0/1"),
            ),
            (
                "interface tengigabitethernet1/0/1",
                Some("interface:tengigabitethernet:1/0/1"),
            ),
            (
                "interface TwentyFiveGigE1/0/1",
                Some("interface:twentyfivegige:1/0/1"),
            ),
            (
                "interface twentyfivegige1/0/1",
                Some("interface:twentyfivegige:1/0/1"),
            ),
            (
                "interface FortyGigabitEthernet1/0/1",
                Some("interface:fortygigabitethernet:1/0/1"),
            ),
            (
                "interface HundredGigE1/0/1",
                Some("interface:hundredgige:1/0/1"),
            ),
            (
                "interface hundredgige1/0/1",
                Some("interface:hundredgige:1/0/1"),
            ),
            (
                "interface TwoGigabitEthernet1/0/1",
                Some("interface:twogigabitethernet:1/0/1"),
            ),
            (
                "interface FiveGigabitEthernet1/0/1",
                Some("interface:fivegigabitethernet:1/0/1"),
            ),
            (
                "interface AppGigabitEthernet1/0/1",
                Some("interface:appgigabitethernet:1/0/1"),
            ),
            (
                "interface FastEthernet0/0",
                Some("interface:fastethernet:0/0"),
            ),
            (
                "interface fastethernet0/0",
                Some("interface:fastethernet:0/0"),
            ),
            (
                "interface Port-channel10",
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
            ("interface Tunnel0", Some("interface:tunnel:0")),
            ("interface tunnel0", Some("interface:tunnel:0")),
            ("interface Serial0/0/0", Some("interface:serial:0/0/0")),
            ("interface BDI100", Some("interface:bdi:100")),
            ("interface bdi100", Some("interface:bdi:100")),
            // unknown types fall back to the raw name
            ("interface Dialer1", Some("interface:Dialer1")),
            ("vrf definition MGMT", Some("vrf:MGMT")),
            // the trustpoint rule must not swallow the certificate chain
            (
                "crypto pki trustpoint MY-CA",
                Some("crypto:pki:trustpoint:MY-CA"),
            ),
            (
                "crypto pki certificate chain MY-CA",
                Some("crypto:pki:certificate-chain:MY-CA"),
            ),
            ("redundancy", Some("redundancy")),
            (
                "parameter-map type inspect GLOBAL-INSPECT",
                Some("parameter-map:inspect:GLOBAL-INSPECT"),
            ),
            (
                "parameter-map type regex MATCH-URL",
                Some("parameter-map:regex:MATCH-URL"),
            ),
            ("parameter-map name MYMAP", None),
            ("track 1 ip sla 1", Some("track:1")),
            (
                "track 10 interface GigabitEthernet0/0/0 line-protocol",
                Some("track:10"),
            ),
            ("zone security INSIDE", Some("zone-security:INSIDE")),
            ("zone something-else", None),
            (
                "zone-pair security ZP-IN-OUT source INSIDE destination OUTSIDE",
                Some("zone-pair:ZP-IN-OUT"),
            ),
            ("zone-pair other", None),
        ] {
            assert_eq!(hint(line).as_deref(), expected, "{line}");
        }
    }

    #[test]
    fn key_hint_crypto_isakmp() {
        assert_eq!(
            hint("crypto isakmp policy 10"),
            Some("crypto:isakmp:policy:10".into()),
        );
    }

    #[test]
    fn parse_round_trip() {
        let cfg = "\
hostname csr-1000v-01
!
vrf definition MGMT
 rd 10.0.0.1:100
 address-family ipv4
  exit-address-family
!
vlan 10
  name SERVERS
vlan 20
  name MGMT
!
track 1 ip sla 1
  delay down 10 up 30
!
interface GigabitEthernet0/0/0
  description WAN-uplink
  ip address 192.0.2.2 255.255.255.252
  no shutdown
interface GigabitEthernet0/0/0.100
  encapsulation dot1Q 100
  ip address 10.10.1.1 255.255.255.0
interface TenGigabitEthernet1/0/1
  description spine-uplink
interface Port-channel10
  description lag-to-peer
interface Loopback0
  ip address 10.255.255.1 255.255.255.255
interface Tunnel0
  ip address 172.16.0.1 255.255.255.252
  tunnel source GigabitEthernet0/0/0
  tunnel destination 198.51.100.1
interface Vlan100
  ip address 10.10.1.1 255.255.255.0
!
router bgp 65000
  bgp router-id 10.255.255.1
router ospf 1
  router-id 10.255.255.1
router eigrp 100
  network 10.0.0.0 0.0.0.255
!
ip access-list extended BLOCK-RFC1918
  deny ip 10.0.0.0 0.255.255.255 any
  deny ip 172.16.0.0 0.15.255.255 any
  deny ip 192.168.0.0 0.0.255.255 any
  permit ip any any
!
crypto pki trustpoint MY-CA
  enrollment url http://ca.example.com
!
redundancy
  mode sso
!
line con 0
  logging synchronous
line vty 0 4
  transport input ssh
";
        let doc = parse(cfg);
        assert_eq!(doc.render(), cfg);
    }
}
