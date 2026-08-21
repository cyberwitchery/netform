use netform_dialect_iosxr::parse_iosxr;
use netform_ir::{DialectHint, Node};

fn hint(line: &str) -> Option<String> {
    let doc = parse_iosxr(&format!("{line}\n"));
    match doc.node(doc.roots[0]).expect("node in arena") {
        Node::Line(l) => l.key_hint.clone(),
        Node::Block(b) => b.header.key_hint.clone(),
    }
}

#[test]
fn parse_iosxr_sets_named_dialect_hint() {
    let doc = parse_iosxr("hostname xr-pe-01\n");
    assert_eq!(
        doc.metadata.dialect_hint,
        DialectHint::Named("iosxr".into())
    );
}

#[test]
fn key_hint_interface_four_part_slot() {
    assert_eq!(
        hint("interface GigabitEthernet0/0/0/0"),
        Some("interface:gigabitethernet:0/0/0/0".into()),
    );
}

#[test]
fn key_hint_interface_subinterface() {
    assert_eq!(
        hint("interface GigabitEthernet0/0/0/0.100"),
        Some("interface:gigabitethernet:0/0/0/0.100".into()),
    );
}

#[test]
fn key_hint_interface_speed_variants() {
    assert_eq!(
        hint("interface TenGigE0/0/0/1"),
        Some("interface:tengige:0/0/0/1".into()),
    );
    assert_eq!(
        hint("interface FortyGigE0/0/0/2"),
        Some("interface:fortygige:0/0/0/2".into()),
    );
    assert_eq!(
        hint("interface HundredGigE0/0/0/3"),
        Some("interface:hundredgige:0/0/0/3".into()),
    );
}

#[test]
fn key_hint_interface_xr_specific_types() {
    assert_eq!(
        hint("interface Bundle-Ether10"),
        Some("interface:bundle-ether:10".into()),
    );
    assert_eq!(
        hint("interface MgmtEth0/RP0/CPU0/0"),
        Some("interface:mgmteth:0/RP0/CPU0/0".into()),
    );
    assert_eq!(
        hint("interface tunnel-ip1"),
        Some("interface:tunnel-ip:1".into()),
    );
    assert_eq!(hint("interface BVI100"), Some("interface:bvi:100".into()));
    assert_eq!(
        hint("interface PW-Ether5"),
        Some("interface:pw-ether:5".into()),
    );
    assert_eq!(
        hint("interface Loopback0"),
        Some("interface:loopback:0".into())
    );
    assert_eq!(hint("interface nve1"), Some("interface:nve:1".into()));
}

#[test]
fn key_hint_interface_type_is_case_insensitive() {
    assert_eq!(
        hint("interface bundle-ether10"),
        Some("interface:bundle-ether:10".into()),
    );
    assert_eq!(
        hint("interface TENGIGE0/0/0/1"),
        Some("interface:tengige:0/0/0/1".into()),
    );
}

#[test]
fn key_hint_interface_unknown_type_falls_back_to_the_raw_name() {
    assert_eq!(
        hint("interface Ethernet1/1"),
        Some("interface:Ethernet1/1".into()),
    );
}

#[test]
fn key_hint_interface_bare_has_no_hint() {
    assert_eq!(hint("interface"), None);
}

#[test]
fn key_hint_vrf_uses_the_bare_form() {
    assert_eq!(hint("vrf CUSTOMER-A"), Some("vrf:CUSTOMER-A".into()));
}

#[test]
fn key_hint_router_protocols() {
    assert_eq!(hint("router bgp 65001"), Some("router:bgp:65001".into()));
    assert_eq!(hint("router ospf CORE"), Some("router:ospf:CORE".into()));
    assert_eq!(hint("router isis 1"), Some("router:isis:1".into()));
    assert_eq!(hint("router static"), Some("router:static".into()));
}

#[test]
fn key_hint_route_policy() {
    assert_eq!(
        hint("route-policy CUSTOMER-IN"),
        Some("route-policy:CUSTOMER-IN".into()),
    );
}

#[test]
fn key_hint_set_families() {
    assert_eq!(
        hint("prefix-set CUSTOMER-PFX"),
        Some("prefix-set:CUSTOMER-PFX".into()),
    );
    assert_eq!(
        hint("as-path-set TRANSIT"),
        Some("as-path-set:TRANSIT".into()),
    );
    assert_eq!(
        hint("community-set NO-EXPORT-SET"),
        Some("community-set:NO-EXPORT-SET".into()),
    );
    assert_eq!(
        hint("extcommunity-set rt CUSTOMER-RT"),
        Some("extcommunity-set:rt:CUSTOMER-RT".into()),
    );
    assert_eq!(hint("rd-set CORE-RD"), Some("rd-set:CORE-RD".into()));
}

#[test]
fn key_hint_bgp_group_families() {
    assert_eq!(
        hint("neighbor-group CUSTOMER-V4"),
        Some("neighbor-group:CUSTOMER-V4".into()),
    );
    assert_eq!(
        hint("af-group TRANSIT-V4"),
        Some("af-group:TRANSIT-V4".into())
    );
    assert_eq!(
        hint("session-group PEER-COMMON"),
        Some("session-group:PEER-COMMON".into()),
    );
}

#[test]
fn key_hint_bare_set_or_group_head_has_no_hint() {
    for line in [
        "route-policy",
        "prefix-set",
        "as-path-set",
        "community-set",
        "extcommunity-set",
        "rd-set",
        "neighbor-group",
        "af-group",
        "session-group",
    ] {
        assert_eq!(hint(line), None, "{line} should not key");
    }
}

#[test]
fn key_hint_ipv4_access_list() {
    assert_eq!(
        hint("ipv4 access-list ACL-EDGE-IN"),
        Some("ipv4-access-list:ACL-EDGE-IN".into()),
    );
}

#[test]
fn key_hint_ipv4_address_is_not_keyed() {
    assert_eq!(hint("ipv4 address 192.0.2.1 255.255.255.252"), None);
}

#[test]
fn key_hint_falls_back_to_the_shared_arms() {
    assert_eq!(
        hint("ipv6 access-list ACL6-IN"),
        Some("ipv6-access-list:ACL6-IN".into()),
    );
    assert_eq!(
        hint("line template MGMT"),
        Some("line:template:MGMT".into()),
    );
    assert_eq!(
        hint("ntp server 10.0.0.1"),
        Some("ntp:server:10.0.0.1".into()),
    );
    assert_eq!(
        hint("policy-map PARENT-SHAPER"),
        Some("policy-map:PARENT-SHAPER".into()),
    );
}

#[test]
fn key_hint_none_for_unkeyed_lines() {
    assert_eq!(hint("hostname xr-pe-01"), None);
    assert_eq!(hint("commit"), None);
}

#[test]
fn comments_and_blank_lines_are_never_keyed() {
    let doc = parse_iosxr("!! IOS XR Configuration 7.3.2\n!\n\n");
    for id in &doc.roots {
        let Node::Line(line) = doc.node(*id).expect("node in arena") else {
            panic!("comment or blank should not open a block");
        };
        assert_eq!(line.key_hint, None);
    }
}
