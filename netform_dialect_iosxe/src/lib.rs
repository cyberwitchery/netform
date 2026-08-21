//! Cisco IOS XE-oriented dialect profile for `netform_ir`.
//!
//! this crate provides [`parse_iosxe`] and the reusable [`IOSXE_DIALECT`]
//! profile, which customize key-hint derivation for IOS XE-specific constructs
//! while reusing the shared IOS-like trivia classification and line tokenization.
//!
//! # Example
//!
//! ```rust
//! use netform_dialect_iosxe::parse_iosxe;
//!
//! let cfg = "interface GigabitEthernet0/0/0\n  description \"WAN uplink\"\n";
//! let doc = parse_iosxe(cfg);
//! assert_eq!(doc.render(), cfg);
//! ```

use netform_ir::{
    Document, IosKeyHintConfig, IosLikeDialect, ParsedLineParts, common_key_hint,
    ios_family_key_hint, parse_with_dialect,
};

/// pre-built IOS XE dialect profile: IOS-like parsing with IOS XE-specific key hints.
pub const IOSXE_DIALECT: IosLikeDialect = IosLikeDialect::new("iosxe", iosxe_key_hint);

/// parse text using the IOS XE dialect ([`IOSXE_DIALECT`]).
pub fn parse_iosxe(input: &str) -> Document {
    parse_with_dialect(input, &IOSXE_DIALECT)
}

/// IOS XE interface type prefixes in canonical lowercase form.
///
/// longest-prefix-first (see `parse_interface`).
///
/// public so `netform_cli`'s `detect_guard_coverage` suite can assert that no
/// entry here is read as IOS XR on slot shape alone; adding a type without
/// widening `netform_ir::detect`'s guard changes what `--dialect auto` reports.
pub const IOSXE_INTERFACE_TYPES: &[&str] = &[
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
];

/// IOS XE-specific configuration for [`ios_family_key_hint`].
const IOSXE_KEY_HINT_CONFIG: IosKeyHintConfig = IosKeyHintConfig {
    interface_types: IOSXE_INTERFACE_TYPES,
    vrf_keyword: "definition",
    extra_router_protos: &["eigrp", "isis"],
};

/// derive a stable identity key for IOS XE configuration lines.
///
/// delegates `interface`, `vrf`, `router`, and `ip` to
/// [`ios_family_key_hint`], handles IOS XE-specific constructs (`crypto pki`,
/// `redundancy`, `parameter-map`, `track`, `zone`, `zone-pair`), then falls
/// back to [`common_key_hint`] for the remaining shared arms.
fn iosxe_key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    if let Some(hint) = ios_family_key_hint(parsed, &IOSXE_KEY_HINT_CONFIG) {
        return Some(hint);
    }

    let parsed_ref = parsed?;
    let head = parsed_ref.head.as_str();
    let args = parsed_ref.args.as_slice();

    match head {
        "crypto" => match args {
            [kind, sub1, sub2, name, ..]
                if kind == "pki" && sub1 == "certificate" && sub2 == "chain" =>
            {
                Some(format!("crypto:pki:certificate-chain:{name}"))
            }
            [kind, sub, name, ..] if kind == "pki" => Some(format!("crypto:pki:{sub}:{name}")),
            _ => common_key_hint(parsed),
        },
        "redundancy" => Some("redundancy".into()),
        "parameter-map" => match args {
            [sub, kind, name, ..] if sub == "type" => Some(format!("parameter-map:{kind}:{name}")),
            _ => None,
        },
        "track" => args.first().map(|num| format!("track:{num}")),
        "zone" => match args {
            [sub, name, ..] if sub == "security" => Some(format!("zone-security:{name}")),
            _ => None,
        },
        "zone-pair" => match args {
            [sub, name, ..] if sub == "security" => Some(format!("zone-pair:{name}")),
            _ => None,
        },
        _ => common_key_hint(parsed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::{
        DialectHint, Node, TriviaKind, classify_ios_like_trivia, parse_ios_like_parts,
    };

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
        let doc = parse_iosxe(
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
        let doc = parse_iosxe(
            "banner motd ^C\ninterface GigabitEthernet0/0/0\n^C\ninterface GigabitEthernet0/0/0\n",
        );

        let hints: Vec<_> = root_lines(&doc)
            .into_iter()
            .filter_map(|(_, _, hint)| hint.map(str::to_string))
            .collect();
        assert_eq!(hints, vec!["interface:gigabitethernet:0/0/0"]);
    }

    #[test]
    fn iosxe_comment_classification_supports_bang_and_hash() {
        assert_eq!(classify_ios_like_trivia("!"), TriviaKind::Comment);
        assert_eq!(classify_ios_like_trivia("# generated"), TriviaKind::Comment);
        assert_eq!(
            classify_ios_like_trivia("interface GigabitEthernet0/0/0"),
            TriviaKind::Content
        );
    }

    #[test]
    fn iosxe_tokenization_keeps_quoted_values_together() {
        let parsed =
            parse_ios_like_parts("description \"WAN uplink\"").expect("content should parse");
        assert_eq!(parsed.head, "description");
        assert_eq!(parsed.args, vec!["\"WAN uplink\""]);
    }

    #[test]
    fn parse_iosxe_sets_named_dialect_hint() {
        let doc = parse_iosxe("hostname edge-1\n");
        assert_eq!(
            doc.metadata.dialect_hint,
            DialectHint::Named("iosxe".into())
        );
    }

    fn hint(line: &str) -> Option<String> {
        let parsed = parse_ios_like_parts(line);
        iosxe_key_hint(parsed.as_ref())
    }

    #[test]
    fn key_hint_interface_gigabitethernet() {
        assert_eq!(
            hint("interface GigabitEthernet0/0/0"),
            Some("interface:gigabitethernet:0/0/0".into()),
        );
    }

    #[test]
    fn key_hint_interface_gigabitethernet_slot_port() {
        assert_eq!(
            hint("interface GigabitEthernet1/0/1"),
            Some("interface:gigabitethernet:1/0/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_gigabitethernet_subinterface() {
        assert_eq!(
            hint("interface GigabitEthernet0/0/0.100"),
            Some("interface:gigabitethernet:0/0/0.100".into()),
        );
    }

    #[test]
    fn key_hint_interface_gigabitethernet_lowercase() {
        assert_eq!(
            hint("interface gigabitethernet0/0/0"),
            Some("interface:gigabitethernet:0/0/0".into()),
        );
    }

    #[test]
    fn key_hint_interface_gigabitethernet_allcaps() {
        assert_eq!(
            hint("interface GIGABITETHERNET0/0/0"),
            Some("interface:gigabitethernet:0/0/0".into()),
        );
    }

    #[test]
    fn key_hint_interface_tengigabitethernet() {
        assert_eq!(
            hint("interface TenGigabitEthernet1/0/1"),
            Some("interface:tengigabitethernet:1/0/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_tengigabitethernet_lowercase() {
        assert_eq!(
            hint("interface tengigabitethernet1/0/1"),
            Some("interface:tengigabitethernet:1/0/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_twentyfivegige() {
        assert_eq!(
            hint("interface TwentyFiveGigE1/0/1"),
            Some("interface:twentyfivegige:1/0/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_twentyfivegige_lowercase() {
        assert_eq!(
            hint("interface twentyfivegige1/0/1"),
            Some("interface:twentyfivegige:1/0/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_fortygigabitethernet() {
        assert_eq!(
            hint("interface FortyGigabitEthernet1/0/1"),
            Some("interface:fortygigabitethernet:1/0/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_hundredgige() {
        assert_eq!(
            hint("interface HundredGigE1/0/1"),
            Some("interface:hundredgige:1/0/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_hundredgige_lowercase() {
        assert_eq!(
            hint("interface hundredgige1/0/1"),
            Some("interface:hundredgige:1/0/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_twogigabitethernet() {
        assert_eq!(
            hint("interface TwoGigabitEthernet1/0/1"),
            Some("interface:twogigabitethernet:1/0/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_fivegigabitethernet() {
        assert_eq!(
            hint("interface FiveGigabitEthernet1/0/1"),
            Some("interface:fivegigabitethernet:1/0/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_appgigabitethernet() {
        assert_eq!(
            hint("interface AppGigabitEthernet1/0/1"),
            Some("interface:appgigabitethernet:1/0/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_fastethernet() {
        assert_eq!(
            hint("interface FastEthernet0/0"),
            Some("interface:fastethernet:0/0".into()),
        );
    }

    #[test]
    fn key_hint_interface_fastethernet_lowercase() {
        assert_eq!(
            hint("interface fastethernet0/0"),
            Some("interface:fastethernet:0/0".into()),
        );
    }

    #[test]
    fn key_hint_interface_port_channel() {
        assert_eq!(
            hint("interface Port-channel10"),
            Some("interface:port-channel:10".into()),
        );
    }

    #[test]
    fn key_hint_interface_port_channel_lowercase() {
        assert_eq!(
            hint("interface port-channel10"),
            Some("interface:port-channel:10".into()),
        );
    }

    #[test]
    fn key_hint_interface_vlan() {
        assert_eq!(hint("interface Vlan100"), Some("interface:vlan:100".into()));
    }

    #[test]
    fn key_hint_interface_vlan_lowercase() {
        assert_eq!(hint("interface vlan100"), Some("interface:vlan:100".into()));
    }

    #[test]
    fn key_hint_interface_loopback() {
        assert_eq!(
            hint("interface Loopback0"),
            Some("interface:loopback:0".into()),
        );
    }

    #[test]
    fn key_hint_interface_loopback_lowercase() {
        assert_eq!(
            hint("interface loopback0"),
            Some("interface:loopback:0".into()),
        );
    }

    #[test]
    fn key_hint_interface_tunnel() {
        assert_eq!(hint("interface Tunnel0"), Some("interface:tunnel:0".into()),);
    }

    #[test]
    fn key_hint_interface_tunnel_lowercase() {
        assert_eq!(hint("interface tunnel0"), Some("interface:tunnel:0".into()),);
    }

    #[test]
    fn key_hint_interface_serial() {
        assert_eq!(
            hint("interface Serial0/0/0"),
            Some("interface:serial:0/0/0".into()),
        );
    }

    #[test]
    fn key_hint_interface_bdi() {
        assert_eq!(hint("interface BDI100"), Some("interface:bdi:100".into()));
    }

    #[test]
    fn key_hint_interface_bdi_lowercase() {
        assert_eq!(hint("interface bdi100"), Some("interface:bdi:100".into()));
    }

    #[test]
    fn key_hint_interface_unknown_type() {
        assert_eq!(hint("interface Dialer1"), Some("interface:Dialer1".into()),);
    }

    #[test]
    fn key_hint_interface_bare_no_hint() {
        assert_eq!(hint("interface"), None);
    }

    #[test]
    fn key_hint_vlan_single() {
        assert_eq!(hint("vlan 100"), Some("vlan:100".into()));
    }

    #[test]
    fn key_hint_vlan_range() {
        assert_eq!(
            hint("vlan 1-100,200,300-400"),
            Some("vlan:1-100,200,300-400".into()),
        );
    }

    #[test]
    fn key_hint_vrf_definition() {
        assert_eq!(hint("vrf definition MGMT"), Some("vrf:MGMT".into()));
    }

    #[test]
    fn key_hint_vrf_bare() {
        assert_eq!(hint("vrf MGMT"), Some("vrf:MGMT".into()));
    }

    #[test]
    fn key_hint_router_bgp() {
        assert_eq!(hint("router bgp 65000"), Some("router:bgp:65000".into()));
    }

    #[test]
    fn key_hint_router_ospf_with_process_id() {
        assert_eq!(hint("router ospf 1"), Some("router:ospf:1".into()));
    }

    #[test]
    fn key_hint_router_ospf_without_process_id() {
        assert_eq!(hint("router ospf"), Some("router:ospf".into()));
    }

    #[test]
    fn key_hint_router_eigrp_with_as() {
        assert_eq!(hint("router eigrp 100"), Some("router:eigrp:100".into()));
    }

    #[test]
    fn key_hint_router_eigrp_named() {
        assert_eq!(
            hint("router eigrp ENTERPRISE"),
            Some("router:eigrp:ENTERPRISE".into()),
        );
    }

    #[test]
    fn key_hint_router_isis() {
        assert_eq!(hint("router isis"), Some("router:isis".into()));
    }

    #[test]
    fn key_hint_router_isis_with_tag() {
        assert_eq!(
            hint("router isis AREA-A"),
            Some("router:isis:AREA-A".into()),
        );
    }

    #[test]
    fn key_hint_route_map_full() {
        assert_eq!(
            hint("route-map REDISTRIBUTE permit 10"),
            Some("route-map:REDISTRIBUTE:permit:10".into()),
        );
    }

    #[test]
    fn key_hint_route_map_no_seq() {
        assert_eq!(
            hint("route-map EXPORT deny"),
            Some("route-map:EXPORT:deny".into()),
        );
    }

    #[test]
    fn key_hint_ip_access_list_bare() {
        assert_eq!(
            hint("ip access-list ACL-MGMT"),
            Some("ip-access-list:ACL-MGMT".into()),
        );
    }

    #[test]
    fn key_hint_ip_access_list_extended() {
        assert_eq!(
            hint("ip access-list extended BLOCK-RFC1918"),
            Some("ip-access-list:extended:BLOCK-RFC1918".into()),
        );
    }

    #[test]
    fn key_hint_ip_access_list_standard() {
        assert_eq!(
            hint("ip access-list standard ALLOW-SNMP"),
            Some("ip-access-list:standard:ALLOW-SNMP".into()),
        );
    }

    #[test]
    fn key_hint_ip_prefix_list() {
        assert_eq!(
            hint("ip prefix-list DEFAULT-ONLY"),
            Some("prefix-list:DEFAULT-ONLY".into()),
        );
    }

    #[test]
    fn key_hint_ip_route() {
        assert_eq!(
            hint("ip route 10.0.0.0 255.0.0.0 192.168.1.1"),
            Some("ip-route:10.0.0.0".into()),
        );
    }

    #[test]
    fn key_hint_ip_route_vrf() {
        assert_eq!(
            hint("ip route vrf MGMT 0.0.0.0 0.0.0.0 10.0.0.1"),
            Some("ip-route:MGMT:0.0.0.0".into()),
        );
    }

    #[test]
    fn key_hint_crypto_pki_trustpoint() {
        assert_eq!(
            hint("crypto pki trustpoint MY-CA"),
            Some("crypto:pki:trustpoint:MY-CA".into()),
        );
    }

    #[test]
    fn key_hint_crypto_pki_certificate_chain() {
        assert_eq!(
            hint("crypto pki certificate chain MY-CA"),
            Some("crypto:pki:certificate-chain:MY-CA".into()),
        );
    }

    #[test]
    fn key_hint_crypto_ikev2() {
        assert_eq!(
            hint("crypto ikev2 proposal PROP-1"),
            Some("crypto:ikev2:proposal:PROP-1".into()),
        );
    }

    #[test]
    fn key_hint_crypto_ipsec() {
        assert_eq!(
            hint("crypto ipsec transform-set MY-TSET"),
            Some("crypto:ipsec:transform-set:MY-TSET".into()),
        );
    }

    #[test]
    fn key_hint_crypto_map() {
        assert_eq!(
            hint("crypto map MY-MAP 10"),
            Some("crypto:map:MY-MAP".into()),
        );
    }

    #[test]
    fn key_hint_crypto_isakmp() {
        assert_eq!(
            hint("crypto isakmp policy 10"),
            Some("crypto:isakmp:policy".into()),
        );
    }

    #[test]
    fn key_hint_redundancy() {
        assert_eq!(hint("redundancy"), Some("redundancy".into()));
    }

    #[test]
    fn key_hint_parameter_map() {
        assert_eq!(
            hint("parameter-map type inspect GLOBAL-INSPECT"),
            Some("parameter-map:inspect:GLOBAL-INSPECT".into()),
        );
    }

    #[test]
    fn key_hint_parameter_map_regex() {
        assert_eq!(
            hint("parameter-map type regex MATCH-URL"),
            Some("parameter-map:regex:MATCH-URL".into()),
        );
    }

    #[test]
    fn key_hint_parameter_map_no_type() {
        assert_eq!(hint("parameter-map name MYMAP"), None);
    }

    #[test]
    fn key_hint_track() {
        assert_eq!(hint("track 1 ip sla 1"), Some("track:1".into()));
        assert_eq!(
            hint("track 10 interface GigabitEthernet0/0/0 line-protocol"),
            Some("track:10".into()),
        );
    }

    #[test]
    fn key_hint_zone_security() {
        assert_eq!(
            hint("zone security INSIDE"),
            Some("zone-security:INSIDE".into()),
        );
    }

    #[test]
    fn key_hint_zone_no_security() {
        assert_eq!(hint("zone something-else"), None);
    }

    #[test]
    fn key_hint_zone_pair_security() {
        assert_eq!(
            hint("zone-pair security ZP-IN-OUT source INSIDE destination OUTSIDE"),
            Some("zone-pair:ZP-IN-OUT".into()),
        );
    }

    #[test]
    fn key_hint_zone_pair_no_security() {
        assert_eq!(hint("zone-pair other"), None);
    }

    #[test]
    fn key_hint_monitor_session() {
        assert_eq!(hint("monitor session 1"), Some("monitor-session:1".into()));
    }

    #[test]
    fn key_hint_monitor_no_session() {
        assert_eq!(hint("monitor capture CAP1"), None);
    }

    #[test]
    fn key_hint_ntp_server() {
        assert_eq!(
            hint("ntp server 10.0.0.1"),
            Some("ntp:server:10.0.0.1".into()),
        );
    }

    #[test]
    fn key_hint_ntp_peer() {
        assert_eq!(hint("ntp peer 10.0.0.2"), Some("ntp:peer:10.0.0.2".into()));
    }

    #[test]
    fn key_hint_ntp_no_match() {
        assert_eq!(hint("ntp source GigabitEthernet0/0/0"), None);
    }

    #[test]
    fn key_hint_class_map() {
        assert_eq!(
            hint("class-map match-all VOICE"),
            Some("class-map:VOICE".into()),
        );
    }

    #[test]
    fn key_hint_policy_map() {
        assert_eq!(
            hint("policy-map QOS-POLICY"),
            Some("policy-map:QOS-POLICY".into()),
        );
    }

    #[test]
    fn key_hint_spanning_tree_vlan() {
        assert_eq!(
            hint("spanning-tree vlan 1-100 priority 4096"),
            Some("spanning-tree:vlan:1-100".into()),
        );
    }

    #[test]
    fn key_hint_line() {
        assert_eq!(hint("line vty 0 4"), Some("line:vty:0:4".into()));
        assert_eq!(hint("line con 0"), Some("line:con:0".into()));
    }

    #[test]
    fn key_hint_none_for_unknown() {
        assert_eq!(hint("hostname edge-1"), None);
    }

    #[test]
    fn key_hint_none_on_empty() {
        assert_eq!(iosxe_key_hint(None), None);
    }

    #[test]
    fn parse_iosxe_round_trip() {
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
        let doc = parse_iosxe(cfg);
        assert_eq!(doc.render(), cfg);
    }

    #[test]
    fn parsed_document_carries_iosxe_interface_hints() {
        let cfg = "interface GigabitEthernet0/0/0\n  description uplink\n";
        let doc = parse_iosxe(cfg);
        let first = match &doc.arena[doc.roots[0].0] {
            netform_ir::Node::Block(b) => &b.header,
            netform_ir::Node::Line(l) => l,
        };
        assert_eq!(
            first.key_hint.as_deref(),
            Some("interface:gigabitethernet:0/0/0")
        );
    }

    #[test]
    fn parsed_document_carries_iosxe_vrf_hint() {
        let cfg = "vrf definition MGMT\n  rd 10.0.0.1:100\n";
        let doc = parse_iosxe(cfg);
        let first = match &doc.arena[doc.roots[0].0] {
            netform_ir::Node::Block(b) => &b.header,
            netform_ir::Node::Line(l) => l,
        };
        assert_eq!(first.key_hint.as_deref(), Some("vrf:MGMT"));
    }

    #[test]
    fn parsed_document_carries_iosxe_redundancy_hint() {
        let cfg = "redundancy\n  mode sso\n";
        let doc = parse_iosxe(cfg);
        let first = match &doc.arena[doc.roots[0].0] {
            netform_ir::Node::Block(b) => &b.header,
            netform_ir::Node::Line(l) => l,
        };
        assert_eq!(first.key_hint.as_deref(), Some("redundancy"));
    }
}
