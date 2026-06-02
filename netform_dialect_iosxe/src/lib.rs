//! Cisco IOS XE-oriented dialect profile for `netform_ir`.
//!
//! This crate provides a dedicated [`IosxeDialect`] that customizes key-hint
//! derivation for IOS XE-specific constructs while reusing the shared IOS-like
//! trivia classification and line tokenization.
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
    Dialect, DialectHint, Document, ParsedLineParts, TriviaKind, classify_ios_like_trivia,
    common_key_hint, parse_ios_like_parts, parse_with_dialect,
};

/// Dialect implementation for Cisco IOS XE configuration text.
#[derive(Debug, Default, Clone, Copy)]
pub struct IosxeDialect;

/// Pre-built IOS XE dialect instance.
pub const IOSXE_DIALECT: IosxeDialect = IosxeDialect;

/// Parse text using [`IosxeDialect`].
pub fn parse_iosxe(input: &str) -> Document {
    parse_with_dialect(input, &IosxeDialect)
}

impl Dialect for IosxeDialect {
    fn dialect_hint(&self) -> DialectHint {
        DialectHint::Named("iosxe".to_string())
    }

    fn classify_trivia(&self, raw: &str) -> TriviaKind {
        classify_ios_like_trivia(raw)
    }

    fn parse_parts(&self, raw: &str) -> Option<ParsedLineParts> {
        parse_ios_like_parts(raw)
    }

    fn key_hint(
        &self,
        _raw: &str,
        parsed: Option<&ParsedLineParts>,
        trivia: TriviaKind,
    ) -> Option<String> {
        if trivia != TriviaKind::Content {
            return None;
        }
        iosxe_key_hint(parsed)
    }
}

/// IOS XE interface type prefixes in canonical lowercase form.
///
/// Order matters: longer prefixes must come first so that e.g.
/// `tengigabitethernet` matches before `gigabitethernet`.  Matching is
/// case-insensitive so that `GigabitEthernet0/0/0`, `gigabitethernet0/0/0`,
/// and `GIGABITETHERNET0/0/0` all normalize the same way.
const IOSXE_INTERFACE_TYPES: &[&str] = &[
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

/// Parse an IOS XE interface name into `(canonical_type, id)`.
///
/// Uses case-insensitive prefix matching so that any casing of a known
/// interface type normalizes to the canonical lowercase form.
///
/// Returns `None` if the name doesn't match any known IOS XE interface type
/// or has no ID portion after the prefix.
fn parse_iosxe_interface(name: &str) -> Option<(&'static str, &str)> {
    let lower = name.to_ascii_lowercase();
    for &canonical in IOSXE_INTERFACE_TYPES {
        if lower.starts_with(canonical) && name.len() > canonical.len() {
            let id = &name[canonical.len()..];
            return Some((canonical, id));
        }
    }
    None
}

/// Derive a stable identity key for IOS XE configuration lines.
///
/// Handles IOS XE-specific constructs first, then delegates to
/// [`common_key_hint`] for shared IOS-like arms.
///
/// IOS XE-specific enhancements:
/// - Interface type normalization (`GigabitEthernet0/0/0` →
///   `interface:gigabitethernet:0/0/0`)
/// - `vrf definition` syntax (IOS XE uses `vrf definition NAME`)
/// - `router ospf` and `router eigrp` with process/AS identifiers
/// - `ip access-list` bare form (without `extended`/`standard`)
/// - `crypto pki` trustpoints and certificate chains
/// - `redundancy`, `parameter-map`, `track`, `zone security`,
///   `zone-pair security`
fn iosxe_key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    let parsed_ref = parsed?;
    let head = parsed_ref.head.as_str();
    let args = parsed_ref.args.as_slice();

    match head {
        "interface" => {
            let name = args.first()?;
            if let Some((itype, id)) = parse_iosxe_interface(name) {
                Some(format!("interface:{itype}:{id}"))
            } else {
                Some(format!("interface:{name}"))
            }
        }
        "vrf" => match args {
            [sub, name, ..] if sub == "definition" => Some(format!("vrf:{name}")),
            [name, ..] => Some(format!("vrf:{name}")),
            _ => None,
        },
        "router" => match args {
            [proto, asn, ..] if proto == "bgp" => Some(format!("router:bgp:{asn}")),
            [proto, id, ..] if proto == "ospf" => Some(format!("router:ospf:{id}")),
            [proto, id, ..] if proto == "eigrp" => Some(format!("router:eigrp:{id}")),
            [proto, ..] => Some(format!("router:{proto}")),
            _ => None,
        },
        "ip" => match args {
            [next, kind, name, ..] if next == "access-list" => {
                Some(format!("ip-access-list:{kind}:{name}"))
            }
            [next, name] if next == "access-list" => Some(format!("ip-access-list:{name}")),
            [next, name, ..] if next == "prefix-list" => Some(format!("prefix-list:{name}")),
            [next, kind, name, ..] if next == "community-list" => {
                Some(format!("ip-community-list:{kind}:{name}"))
            }
            [next, vrf_kw, vrf_name, prefix, ..] if next == "route" && vrf_kw == "vrf" => {
                Some(format!("ip-route:{vrf_name}:{prefix}"))
            }
            [next, prefix, ..] if next == "route" => Some(format!("ip-route:{prefix}")),
            _ => None,
        },
        // -- IOS XE-specific constructs --
        "crypto" => match args {
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
    use netform_ir::{DialectHint, TriviaKind, classify_ios_like_trivia, parse_ios_like_parts};

    // -- trivia classification (inherited from IOS-like) --

    #[test]
    fn iosxe_comment_classification_supports_bang_and_hash() {
        assert_eq!(classify_ios_like_trivia("!"), TriviaKind::Comment);
        assert_eq!(classify_ios_like_trivia("# generated"), TriviaKind::Comment);
        assert_eq!(
            classify_ios_like_trivia("interface GigabitEthernet0/0/0"),
            TriviaKind::Content
        );
    }

    // -- tokenization (inherited from IOS-like) --

    #[test]
    fn iosxe_tokenization_keeps_quoted_values_together() {
        let parsed =
            parse_ios_like_parts("description \"WAN uplink\"").expect("content should parse");
        assert_eq!(parsed.head, "description");
        assert_eq!(parsed.args, vec!["\"WAN uplink\""]);
    }

    // -- dialect hint --

    #[test]
    fn parse_iosxe_sets_named_dialect_hint() {
        let doc = parse_iosxe("hostname edge-1\n");
        assert_eq!(
            doc.metadata.dialect_hint,
            DialectHint::Named("iosxe".into())
        );
    }

    // -- key hint helper --

    fn hint(line: &str) -> Option<String> {
        let parsed = parse_ios_like_parts(line);
        iosxe_key_hint(parsed.as_ref())
    }

    // -- IOS XE interface type normalization --

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

    // -- vlan --

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

    // -- vrf --

    #[test]
    fn key_hint_vrf_definition() {
        assert_eq!(hint("vrf definition MGMT"), Some("vrf:MGMT".into()));
    }

    #[test]
    fn key_hint_vrf_bare() {
        assert_eq!(hint("vrf MGMT"), Some("vrf:MGMT".into()));
    }

    // -- router --

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

    // -- route-map --

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

    // -- ip access-list --

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

    // -- ip prefix-list --

    #[test]
    fn key_hint_ip_prefix_list() {
        assert_eq!(
            hint("ip prefix-list DEFAULT-ONLY"),
            Some("prefix-list:DEFAULT-ONLY".into()),
        );
    }

    // -- ip route --

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

    // -- IOS XE-specific: crypto pki --

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
            Some("crypto:pki:certificate:chain".into()),
        );
    }

    // -- crypto (common arms still work) --

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

    // -- IOS XE-specific: redundancy --

    #[test]
    fn key_hint_redundancy() {
        assert_eq!(hint("redundancy"), Some("redundancy".into()));
    }

    // -- IOS XE-specific: parameter-map --

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

    // -- IOS XE-specific: track --

    #[test]
    fn key_hint_track() {
        assert_eq!(hint("track 1 ip sla 1"), Some("track:1".into()));
        assert_eq!(
            hint("track 10 interface GigabitEthernet0/0/0 line-protocol"),
            Some("track:10".into()),
        );
    }

    // -- IOS XE-specific: zone security --

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

    // -- IOS XE-specific: zone-pair security --

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

    // -- monitor session --

    #[test]
    fn key_hint_monitor_session() {
        assert_eq!(hint("monitor session 1"), Some("monitor-session:1".into()));
    }

    #[test]
    fn key_hint_monitor_no_session() {
        assert_eq!(hint("monitor capture CAP1"), None);
    }

    // -- ntp --

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

    // -- class-map / policy-map --

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

    // -- spanning-tree --

    #[test]
    fn key_hint_spanning_tree_vlan() {
        assert_eq!(
            hint("spanning-tree vlan 1-100 priority 4096"),
            Some("spanning-tree:vlan:1-100".into()),
        );
    }

    // -- line --

    #[test]
    fn key_hint_line() {
        assert_eq!(hint("line vty 0 4"), Some("line:vty:0:4".into()));
        assert_eq!(hint("line con 0"), Some("line:con:0".into()));
    }

    // -- negative cases --

    #[test]
    fn key_hint_none_for_unknown() {
        assert_eq!(hint("hostname edge-1"), None);
    }

    #[test]
    fn key_hint_none_on_empty() {
        assert_eq!(iosxe_key_hint(None), None);
    }

    // -- round-trip parsing --

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

    // -- key hints appear on parsed document nodes --

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
