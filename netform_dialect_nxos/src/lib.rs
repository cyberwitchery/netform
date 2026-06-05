//! Cisco NX-OS-oriented dialect profile for `netform_ir`.
//!
//! This crate provides a dedicated [`NxosDialect`] that customizes key-hint
//! derivation for NX-OS-specific constructs while reusing the shared IOS-like
//! trivia classification and line tokenization.
//!
//! # Example
//!
//! ```rust
//! use netform_dialect_nxos::parse_nxos;
//!
//! let cfg = "interface Ethernet1/1\n  description Uplink\n";
//! let doc = parse_nxos(cfg);
//! assert_eq!(doc.render(), cfg);
//! ```

use netform_ir::{
    Dialect, DialectHint, Document, IosKeyHintConfig, ParsedLineParts, TriviaKind,
    classify_ios_like_trivia, common_key_hint, ios_family_key_hint, parse_ios_like_parts,
    parse_with_dialect,
};

/// Dialect implementation for Cisco NX-OS configuration text.
#[derive(Debug, Default, Clone, Copy)]
pub struct NxosDialect;

/// Pre-built NX-OS dialect instance.
pub const NXOS_DIALECT: NxosDialect = NxosDialect;

/// Parse text using [`NxosDialect`].
pub fn parse_nxos(input: &str) -> Document {
    parse_with_dialect(input, &NxosDialect)
}

impl Dialect for NxosDialect {
    fn dialect_hint(&self) -> DialectHint {
        DialectHint::Named("nxos".to_string())
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
        nxos_key_hint(parsed)
    }
}

/// NX-OS interface type prefixes in canonical lowercase form.
///
/// Order matters: longer prefixes must come first so `port-channel` matches
/// before a hypothetical `port` prefix. Matching is case-insensitive so that
/// `Ethernet1/1`, `ethernet1/1`, and `ETHERNET1/1` all normalize the same way.
const NXOS_INTERFACE_TYPES: &[&str] = &[
    "port-channel",
    "ethernet",
    "loopback",
    "fabric",
    "tunnel",
    "vlan",
    "mgmt",
    "nve",
];

/// NX-OS-specific configuration for [`ios_family_key_hint`].
const NXOS_KEY_HINT_CONFIG: IosKeyHintConfig = IosKeyHintConfig {
    interface_types: NXOS_INTERFACE_TYPES,
    vrf_keyword: "context",
    extra_router_protos: &[],
};

/// Derive a stable identity key for NX-OS configuration lines.
///
/// Delegates `interface`, `vrf`, `router`, and `ip` to
/// [`ios_family_key_hint`], handles NX-OS-specific constructs (`feature`,
/// `vpc`, `role`, `system`), then falls back to [`common_key_hint`] for the
/// remaining shared arms.
fn nxos_key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    if let Some(hint) = ios_family_key_hint(parsed, &NXOS_KEY_HINT_CONFIG) {
        return Some(hint);
    }

    let parsed_ref = parsed?;
    let head = parsed_ref.head.as_str();
    let args = parsed_ref.args.as_slice();

    match head {
        "feature" => args.first().map(|name| format!("feature:{name}")),
        "vpc" => match args {
            [sub, id, ..] if sub == "domain" => Some(format!("vpc-domain:{id}")),
            _ => None,
        },
        "role" => match args {
            [sub, name, ..] if sub == "name" => Some(format!("role:{name}")),
            _ => None,
        },
        "system" => match args {
            [sub, ..] => Some(format!("system:{sub}")),
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
    fn nxos_comment_classification_supports_bang_and_hash() {
        assert_eq!(classify_ios_like_trivia("!"), TriviaKind::Comment);
        assert_eq!(classify_ios_like_trivia("# generated"), TriviaKind::Comment);
        assert_eq!(
            classify_ios_like_trivia("interface Ethernet1/1"),
            TriviaKind::Content
        );
    }

    // -- tokenization (inherited from IOS-like) --

    #[test]
    fn nxos_tokenization_keeps_quoted_values_together() {
        let parsed =
            parse_ios_like_parts("description \"Uplink to spine\"").expect("content should parse");
        assert_eq!(parsed.head, "description");
        assert_eq!(parsed.args, vec!["\"Uplink to spine\""]);
    }

    // -- dialect hint --

    #[test]
    fn parse_nxos_sets_named_dialect_hint() {
        let doc = parse_nxos("hostname n9k-leaf-01\n");
        assert_eq!(doc.metadata.dialect_hint, DialectHint::Named("nxos".into()));
    }

    // -- key hint helper --

    fn hint(line: &str) -> Option<String> {
        let parsed = parse_ios_like_parts(line);
        nxos_key_hint(parsed.as_ref())
    }

    // -- NX-OS interface type normalization --

    #[test]
    fn key_hint_interface_ethernet_slot_port() {
        assert_eq!(
            hint("interface Ethernet1/1"),
            Some("interface:ethernet:1/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_ethernet_fex() {
        assert_eq!(
            hint("interface Ethernet1/1/1"),
            Some("interface:ethernet:1/1/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_ethernet_subinterface() {
        assert_eq!(
            hint("interface Ethernet1/1.100"),
            Some("interface:ethernet:1/1.100".into()),
        );
    }

    #[test]
    fn key_hint_interface_ethernet_lowercase() {
        assert_eq!(
            hint("interface ethernet1/1"),
            Some("interface:ethernet:1/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_port_channel() {
        assert_eq!(
            hint("interface port-channel10"),
            Some("interface:port-channel:10".into()),
        );
    }

    #[test]
    fn key_hint_interface_port_channel_capitalized() {
        assert_eq!(
            hint("interface Port-channel10"),
            Some("interface:port-channel:10".into()),
        );
    }

    #[test]
    fn key_hint_interface_vlan() {
        assert_eq!(hint("interface Vlan100"), Some("interface:vlan:100".into()),);
    }

    #[test]
    fn key_hint_interface_vlan_lowercase() {
        assert_eq!(hint("interface vlan100"), Some("interface:vlan:100".into()),);
    }

    #[test]
    fn key_hint_interface_loopback() {
        assert_eq!(
            hint("interface loopback0"),
            Some("interface:loopback:0".into()),
        );
    }

    #[test]
    fn key_hint_interface_loopback_capitalized() {
        assert_eq!(
            hint("interface Loopback0"),
            Some("interface:loopback:0".into()),
        );
    }

    #[test]
    fn key_hint_interface_mgmt() {
        assert_eq!(hint("interface mgmt0"), Some("interface:mgmt:0".into()),);
    }

    #[test]
    fn key_hint_interface_nve() {
        assert_eq!(hint("interface nve1"), Some("interface:nve:1".into()),);
    }

    #[test]
    fn key_hint_interface_nve_capitalized() {
        assert_eq!(hint("interface Nve1"), Some("interface:nve:1".into()),);
    }

    #[test]
    fn key_hint_interface_tunnel() {
        assert_eq!(hint("interface tunnel0"), Some("interface:tunnel:0".into()),);
    }

    #[test]
    fn key_hint_interface_tunnel_capitalized() {
        assert_eq!(hint("interface Tunnel1"), Some("interface:tunnel:1".into()),);
    }

    #[test]
    fn key_hint_interface_fabric() {
        assert_eq!(
            hint("interface fabric1/1"),
            Some("interface:fabric:1/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_fabric_capitalized() {
        assert_eq!(
            hint("interface Fabric1/1"),
            Some("interface:fabric:1/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_ethernet_allcaps() {
        assert_eq!(
            hint("interface ETHERNET1/1"),
            Some("interface:ethernet:1/1".into()),
        );
    }

    #[test]
    fn key_hint_interface_unknown_type() {
        // Unknown types fall back to raw name.
        assert_eq!(
            hint("interface GigabitEthernet0/0/0"),
            Some("interface:GigabitEthernet0/0/0".into()),
        );
    }

    #[test]
    fn key_hint_interface_bare_no_hint() {
        assert_eq!(hint("interface"), None);
    }

    // -- vlan (including ranges) --

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
    fn key_hint_vrf_context() {
        assert_eq!(hint("vrf context MGMT"), Some("vrf:MGMT".into()));
    }

    #[test]
    fn key_hint_vrf_bare() {
        assert_eq!(hint("vrf MGMT"), Some("vrf:MGMT".into()));
    }

    // -- router --

    #[test]
    fn key_hint_router_bgp() {
        assert_eq!(hint("router bgp 65001"), Some("router:bgp:65001".into()));
    }

    #[test]
    fn key_hint_router_ospf_with_process_id() {
        assert_eq!(hint("router ospf 1"), Some("router:ospf:1".into()));
    }

    #[test]
    fn key_hint_router_ospf_without_process_id() {
        assert_eq!(hint("router ospf"), Some("router:ospf".into()));
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
    fn key_hint_ip_access_list() {
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

    // -- feature (NX-OS specific) --

    #[test]
    fn key_hint_feature() {
        assert_eq!(hint("feature ospf"), Some("feature:ospf".into()));
        assert_eq!(hint("feature bgp"), Some("feature:bgp".into()));
        assert_eq!(hint("feature vpc"), Some("feature:vpc".into()));
        assert_eq!(
            hint("feature interface-vlan"),
            Some("feature:interface-vlan".into()),
        );
    }

    // -- vpc domain (NX-OS specific) --

    #[test]
    fn key_hint_vpc_domain() {
        assert_eq!(hint("vpc domain 10"), Some("vpc-domain:10".into()));
        assert_eq!(hint("vpc domain 100"), Some("vpc-domain:100".into()));
    }

    #[test]
    fn key_hint_vpc_no_domain() {
        assert_eq!(hint("vpc orphan-ports suspend"), None);
    }

    // -- role name (NX-OS specific) --

    #[test]
    fn key_hint_role_name() {
        assert_eq!(
            hint("role name custom-admin"),
            Some("role:custom-admin".into()),
        );
    }

    #[test]
    fn key_hint_role_no_name() {
        assert_eq!(hint("role feature-group name"), None);
    }

    // -- monitor session (NX-OS specific) --

    #[test]
    fn key_hint_monitor_session() {
        assert_eq!(hint("monitor session 1"), Some("monitor-session:1".into()));
    }

    #[test]
    fn key_hint_monitor_no_session() {
        assert_eq!(hint("monitor copp-system-p-policy"), None);
    }

    // -- ntp (NX-OS specific) --

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
        assert_eq!(hint("ntp source-interface mgmt0"), None);
    }

    // -- system (NX-OS specific) --

    #[test]
    fn key_hint_system() {
        assert_eq!(hint("system jumbomtu 9216"), Some("system:jumbomtu".into()));
        assert_eq!(
            hint("system default switchport"),
            Some("system:default".into()),
        );
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

    // -- crypto --

    #[test]
    fn key_hint_crypto_ikev2() {
        assert_eq!(
            hint("crypto ikev2 proposal PROP-1"),
            Some("crypto:ikev2:proposal:PROP-1".into()),
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
        assert_eq!(hint("hostname ROUTER-1"), None);
    }

    #[test]
    fn key_hint_none_on_empty() {
        assert_eq!(nxos_key_hint(None), None);
    }

    // -- round-trip parsing --

    #[test]
    fn parse_nxos_round_trip() {
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
        let doc = parse_nxos(cfg);
        assert_eq!(doc.render(), cfg);
    }

    // -- key hints appear on parsed document nodes --

    #[test]
    fn parsed_document_carries_nxos_interface_hints() {
        let cfg = "interface Ethernet1/1\n  description uplink\n";
        let doc = parse_nxos(cfg);
        let first = match &doc.arena[doc.roots[0].0] {
            netform_ir::Node::Block(b) => &b.header,
            netform_ir::Node::Line(l) => l,
        };
        assert_eq!(first.key_hint.as_deref(), Some("interface:ethernet:1/1"),);
    }
}
