//! Arista EOS-oriented dialect profile for `netform_ir`.
//!
//! this crate provides [`parse_eos`] and the reusable [`EOS_DIALECT`] profile,
//! which customize key-hint derivation for EOS-specific constructs while reusing
//! the shared IOS-like trivia classification and line tokenization.
//!
//! # Example
//!
//! ```rust
//! use netform_dialect_eos::parse_eos;
//!
//! let cfg = "interface Ethernet1\n   description \"Uplink\"\n";
//! let doc = parse_eos(cfg);
//! assert_eq!(doc.render(), cfg);
//! ```

use netform_ir::{
    Document, IosKeyHintConfig, IosLikeDialect, ParsedLineParts, common_key_hint,
    ios_family_key_hint, parse_with_dialect,
};

/// pre-built EOS dialect profile: IOS-like parsing with EOS-specific key hints.
pub const EOS_DIALECT: IosLikeDialect = IosLikeDialect::new("eos", eos_key_hint);

/// parse text using the EOS dialect ([`EOS_DIALECT`]).
pub fn parse_eos(input: &str) -> Document {
    parse_with_dialect(input, &EOS_DIALECT)
}

/// EOS interface type prefixes in canonical lowercase form.
///
/// longest-prefix-first (see `parse_interface`).
const EOS_INTERFACE_TYPES: &[&str] = &[
    "port-channel",
    "management",
    "ethernet",
    "loopback",
    "vxlan",
    "vlan",
];

/// EOS-specific configuration for [`ios_family_key_hint`].
const EOS_KEY_HINT_CONFIG: IosKeyHintConfig = IosKeyHintConfig {
    interface_types: EOS_INTERFACE_TYPES,
    vrf_keyword: "instance",
    extra_router_protos: &["eigrp", "isis"],
};

/// derive a stable identity key for EOS configuration lines.
///
/// delegates `interface`, `vrf`, `router`, and `ip` to
/// [`ios_family_key_hint`], handles EOS-specific constructs (`mlag`,
/// `management`, `daemon`, `event-handler`, `peer-filter`), then falls back to
/// [`common_key_hint`] for the remaining shared arms.
fn eos_key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    if let Some(hint) = ios_family_key_hint(parsed, &EOS_KEY_HINT_CONFIG) {
        return Some(hint);
    }

    let parsed_ref = parsed?;
    let head = parsed_ref.head.as_str();
    let args = parsed_ref.args.as_slice();

    match head {
        "mlag" => match args {
            [sub, ..] if sub == "configuration" => Some("mlag".into()),
            _ => None,
        },
        "management" => match args {
            [sub, kind, ..] if sub == "api" => Some(format!("management-api:{kind}")),
            [sub, kind, ..] if sub == "ssh" || sub == "telnet" || sub == "console" => {
                Some(format!("management:{sub}:{kind}"))
            }
            _ => None,
        },
        "daemon" => args.first().map(|name| format!("daemon:{name}")),
        "event-handler" => args.first().map(|name| format!("event-handler:{name}")),
        "peer-filter" => args.first().map(|name| format!("peer-filter:{name}")),
        _ => common_key_hint(parsed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::{DialectHint, TriviaKind, classify_ios_like_trivia, parse_ios_like_parts};

    #[test]
    fn eos_comment_classification_supports_bang_and_hash() {
        assert_eq!(classify_ios_like_trivia("!"), TriviaKind::Comment);
        assert_eq!(classify_ios_like_trivia("# generated"), TriviaKind::Comment);
        assert_eq!(
            classify_ios_like_trivia("interface Ethernet1"),
            TriviaKind::Content
        );
    }

    #[test]
    fn eos_tokenization_keeps_quoted_values_together() {
        let parsed =
            parse_ios_like_parts("description \"Transit uplink\"").expect("content should parse");
        assert_eq!(parsed.head, "description");
        assert_eq!(parsed.args, vec!["\"Transit uplink\""]);
    }

    #[test]
    fn parse_eos_sets_named_dialect_hint() {
        let doc = parse_eos("hostname leaf-01\n");
        assert_eq!(doc.metadata.dialect_hint, DialectHint::Named("eos".into()));
    }

    fn hint(line: &str) -> Option<String> {
        let parsed = parse_ios_like_parts(line);
        eos_key_hint(parsed.as_ref())
    }

    #[test]
    fn key_hint_interface_ethernet() {
        assert_eq!(
            hint("interface Ethernet1"),
            Some("interface:ethernet:1".into()),
        );
    }

    #[test]
    fn key_hint_interface_ethernet_modular() {
        assert_eq!(
            hint("interface Ethernet1/1"),
            Some("interface:ethernet:1/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_ethernet_subinterface() {
        assert_eq!(
            hint("interface Ethernet1.100"),
            Some("interface:ethernet:1.100".into()),
        );
    }

    #[test]
    fn key_hint_interface_ethernet_lowercase() {
        assert_eq!(
            hint("interface ethernet1"),
            Some("interface:ethernet:1".into()),
        );
    }

    #[test]
    fn key_hint_interface_ethernet_allcaps() {
        assert_eq!(
            hint("interface ETHERNET1"),
            Some("interface:ethernet:1".into()),
        );
    }

    #[test]
    fn key_hint_interface_port_channel() {
        assert_eq!(
            hint("interface Port-Channel10"),
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
    fn key_hint_interface_management() {
        assert_eq!(
            hint("interface Management1"),
            Some("interface:management:1".into()),
        );
    }

    #[test]
    fn key_hint_interface_management_lowercase() {
        assert_eq!(
            hint("interface management1"),
            Some("interface:management:1".into()),
        );
    }

    #[test]
    fn key_hint_interface_vxlan() {
        assert_eq!(hint("interface Vxlan1"), Some("interface:vxlan:1".into()),);
    }

    #[test]
    fn key_hint_interface_vxlan_lowercase() {
        assert_eq!(hint("interface vxlan1"), Some("interface:vxlan:1".into()),);
    }

    #[test]
    fn key_hint_interface_unknown_type() {
        assert_eq!(
            hint("interface GigabitEthernet0/0/0"),
            Some("interface:GigabitEthernet0/0/0".into()),
        );
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
    fn key_hint_vrf_instance() {
        assert_eq!(hint("vrf instance MGMT"), Some("vrf:MGMT".into()));
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
    fn key_hint_router_isis_bare() {
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
    fn key_hint_ip_prefix_list() {
        assert_eq!(
            hint("ip prefix-list DEFAULT-ONLY"),
            Some("prefix-list:DEFAULT-ONLY".into()),
        );
    }

    #[test]
    fn key_hint_ip_route() {
        assert_eq!(
            hint("ip route 10.0.0.0/8 192.168.1.1"),
            Some("ip-route:10.0.0.0/8".into()),
        );
    }

    #[test]
    fn key_hint_ip_route_vrf() {
        assert_eq!(
            hint("ip route vrf MGMT 0.0.0.0/0 10.0.0.1"),
            Some("ip-route:MGMT:0.0.0.0/0".into()),
        );
    }

    #[test]
    fn key_hint_mlag_configuration() {
        assert_eq!(hint("mlag configuration"), Some("mlag".into()));
    }

    #[test]
    fn key_hint_mlag_no_configuration() {
        assert_eq!(hint("mlag"), None);
    }

    #[test]
    fn key_hint_management_api_http_commands() {
        assert_eq!(
            hint("management api http-commands"),
            Some("management-api:http-commands".into()),
        );
    }

    #[test]
    fn key_hint_management_api_gnmi() {
        assert_eq!(
            hint("management api gnmi"),
            Some("management-api:gnmi".into()),
        );
    }

    #[test]
    fn key_hint_management_api_restful() {
        assert_eq!(
            hint("management api restful"),
            Some("management-api:restful".into()),
        );
    }

    #[test]
    fn key_hint_management_ssh() {
        assert_eq!(
            hint("management ssh idle-timeout 15"),
            Some("management:ssh:idle-timeout".into()),
        );
    }

    #[test]
    fn key_hint_daemon() {
        assert_eq!(hint("daemon TerminAttr"), Some("daemon:TerminAttr".into()));
    }

    #[test]
    fn key_hint_daemon_custom() {
        assert_eq!(hint("daemon myagent"), Some("daemon:myagent".into()),);
    }

    #[test]
    fn key_hint_event_handler() {
        assert_eq!(
            hint("event-handler lnterface-recovery"),
            Some("event-handler:lnterface-recovery".into()),
        );
    }

    #[test]
    fn key_hint_peer_filter() {
        assert_eq!(
            hint("peer-filter LEAF-PEERS"),
            Some("peer-filter:LEAF-PEERS".into()),
        );
    }

    #[test]
    fn key_hint_monitor_session() {
        assert_eq!(hint("monitor session 1"), Some("monitor-session:1".into()));
    }

    #[test]
    fn key_hint_monitor_no_session() {
        assert_eq!(hint("monitor copp-system-p-policy"), None);
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
        assert_eq!(hint("ntp source-interface Management1"), None);
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
    fn key_hint_crypto_ikev2() {
        assert_eq!(
            hint("crypto ikev2 proposal PROP-1"),
            Some("crypto:ikev2:proposal:PROP-1".into()),
        );
    }

    #[test]
    fn key_hint_line() {
        assert_eq!(hint("line vty 0 4"), Some("line:vty:0:4".into()));
        assert_eq!(hint("line con 0"), Some("line:con:0".into()));
    }

    #[test]
    fn key_hint_none_for_unknown() {
        assert_eq!(hint("hostname leaf-01"), None);
    }

    #[test]
    fn key_hint_none_on_empty() {
        assert_eq!(eos_key_hint(None), None);
    }

    #[test]
    fn parse_eos_round_trip() {
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
        let doc = parse_eos(cfg);
        assert_eq!(doc.render(), cfg);
    }

    #[test]
    fn parsed_document_carries_eos_interface_hints() {
        let cfg = "interface Ethernet1\n  description uplink\n";
        let doc = parse_eos(cfg);
        let first = match &doc.arena[doc.roots[0].0] {
            netform_ir::Node::Block(b) => &b.header,
            netform_ir::Node::Line(l) => l,
        };
        assert_eq!(first.key_hint.as_deref(), Some("interface:ethernet:1"));
    }

    #[test]
    fn parsed_document_carries_eos_mlag_hint() {
        let cfg = "mlag configuration\n  domain-id MLAG\n";
        let doc = parse_eos(cfg);
        let first = match &doc.arena[doc.roots[0].0] {
            netform_ir::Node::Block(b) => &b.header,
            netform_ir::Node::Line(l) => l,
        };
        assert_eq!(first.key_hint.as_deref(), Some("mlag"));
    }

    #[test]
    fn parsed_document_carries_eos_management_api_hint() {
        let cfg = "management api http-commands\n  no shutdown\n";
        let doc = parse_eos(cfg);
        let first = match &doc.arena[doc.roots[0].0] {
            netform_ir::Node::Block(b) => &b.header,
            netform_ir::Node::Line(l) => l,
        };
        assert_eq!(
            first.key_hint.as_deref(),
            Some("management-api:http-commands")
        );
    }
}
