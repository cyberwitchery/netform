use netform_dialect_vrp::parse_vrp;
use netform_ir::{DialectHint, Node};

fn hint(line: &str) -> Option<String> {
    let doc = parse_vrp(&format!("{line}\n"));
    match doc.node(doc.roots[0]).expect("node in arena") {
        Node::Line(l) => l.key_hint.clone(),
        Node::Block(b) => b.header.key_hint.clone(),
    }
}

#[test]
fn parse_vrp_sets_named_dialect_hint() {
    let doc = parse_vrp("sysname CE-ACCESS-01\n");
    assert_eq!(doc.metadata.dialect_hint, DialectHint::Named("vrp".into()));
}

#[test]
fn key_hint_interface_vrp_specific_types() {
    assert_eq!(
        hint("interface Vlanif10"),
        Some("interface:vlanif:10".into())
    );
    assert_eq!(
        hint("interface Eth-Trunk1"),
        Some("interface:eth-trunk:1".into()),
    );
    assert_eq!(
        hint("interface Ip-Trunk1"),
        Some("interface:ip-trunk:1".into()),
    );
    assert_eq!(
        hint("interface MEth0/0/1"),
        Some("interface:meth:0/0/1".into())
    );
    assert_eq!(
        hint("interface Virtual-Template1"),
        Some("interface:virtual-template:1".into()),
    );
}

#[test]
fn key_hint_interface_speed_variants() {
    assert_eq!(
        hint("interface GigabitEthernet0/0/1"),
        Some("interface:gigabitethernet:0/0/1".into()),
    );
    assert_eq!(
        hint("interface XGigabitEthernet0/0/1"),
        Some("interface:xgigabitethernet:0/0/1".into()),
    );
    assert_eq!(
        hint("interface 25GE1/0/1"),
        Some("interface:25ge:1/0/1".into())
    );
    assert_eq!(
        hint("interface 40GE1/0/1"),
        Some("interface:40ge:1/0/1".into())
    );
    assert_eq!(
        hint("interface 100GE1/0/1"),
        Some("interface:100ge:1/0/1".into()),
    );
}

#[test]
fn key_hint_interface_normalizes_vendor_casing() {
    assert_eq!(
        hint("interface LoopBack0"),
        Some("interface:loopback:0".into()),
    );
    assert_eq!(hint("interface NULL0"), Some("interface:null:0".into()));
    assert_eq!(
        hint("interface gigabitethernet0/0/1"),
        Some("interface:gigabitethernet:0/0/1".into()),
    );
}

#[test]
fn key_hint_interface_subinterface() {
    assert_eq!(
        hint("interface GigabitEthernet0/0/1.100"),
        Some("interface:gigabitethernet:0/0/1.100".into()),
    );
}

#[test]
fn key_hint_vpn_instance() {
    assert_eq!(
        hint("ip vpn-instance BLUE"),
        Some("vpn-instance:BLUE".into()),
    );
}

#[test]
fn key_hint_acl() {
    assert_eq!(hint("acl number 3000"), Some("acl:3000".into()));
    assert_eq!(hint("acl name MGMT-IN advance"), Some("acl:MGMT-IN".into()));
    assert_eq!(hint("acl 3000"), Some("acl:3000".into()));
}

#[test]
fn key_hint_traffic_families() {
    assert_eq!(
        hint("traffic classifier CLASS-USERS operator or"),
        Some("traffic-classifier:CLASS-USERS".into()),
    );
    assert_eq!(
        hint("traffic behavior BEHAVE-USERS"),
        Some("traffic-behavior:BEHAVE-USERS".into()),
    );
    assert_eq!(
        hint("traffic policy POLICY-EDGE"),
        Some("traffic-policy:POLICY-EDGE".into()),
    );
}

#[test]
fn key_hint_ip_prefix_keys_the_entry_not_just_the_list() {
    assert_eq!(
        hint("ip ip-prefix DEFAULT-ONLY index 10 permit 0.0.0.0 0"),
        Some("ip-prefix:DEFAULT-ONLY:10".into()),
    );
    assert_eq!(
        hint("ip ip-prefix DEFAULT-ONLY index 20 deny 10.0.0.0 8 less-equal 32"),
        Some("ip-prefix:DEFAULT-ONLY:20".into()),
    );
    assert_eq!(
        hint("ip ip-prefix DEFAULT-ONLY permit 0.0.0.0 0"),
        Some("ip-prefix:DEFAULT-ONLY".into()),
    );
}

#[test]
fn key_hint_route_policy_node() {
    assert_eq!(
        hint("route-policy EXPORT-BLUE permit node 10"),
        Some("route-policy:EXPORT-BLUE:permit:10".into()),
    );
    assert_eq!(
        hint("route-policy EXPORT-BLUE deny node 20"),
        Some("route-policy:EXPORT-BLUE:deny:20".into()),
    );
}

#[test]
fn key_hint_user_interface() {
    assert_eq!(
        hint("user-interface vty 0 4"),
        Some("user-interface:vty:0:4".into()),
    );
    assert_eq!(
        hint("user-interface con 0"),
        Some("user-interface:con:0".into()),
    );
}

#[test]
fn key_hint_local_user_keys_the_attribute_too() {
    assert_eq!(
        hint("local-user netops password irreversible-cipher hunter2"),
        Some("local-user:netops:password".into()),
    );
    assert_eq!(
        hint("local-user netops service-type ssh"),
        Some("local-user:netops:service-type".into()),
    );
}

#[test]
fn key_hint_peer_keys_the_attribute_too() {
    assert_eq!(
        hint("peer 10.0.0.2 as-number 65001"),
        Some("peer:10.0.0.2:as-number".into()),
    );
    assert_eq!(
        hint("peer 10.0.0.2 enable"),
        Some("peer:10.0.0.2:enable".into()),
    );
}

#[test]
fn key_hint_router_views() {
    assert_eq!(hint("bgp 65000"), Some("router:bgp:65000".into()));
    assert_eq!(hint("bgp 65000.1"), Some("router:bgp:65000.1".into()));
    assert_eq!(
        hint("ospf 1 router-id 10.255.255.1"),
        Some("router:ospf:1".into())
    );
    assert_eq!(hint("isis 1"), Some("router:isis:1".into()));
}

#[test]
fn key_hint_skips_the_router_keywords_used_as_sub_commands() {
    assert_eq!(hint("ospf network-type p2p"), None);
    assert_eq!(hint("isis enable 1"), None);
}

#[test]
fn key_hint_ip_route_static() {
    assert_eq!(
        hint("ip route-static 0.0.0.0 0.0.0.0 10.0.0.254"),
        Some("ip-route-static:0.0.0.0:0.0.0.0".into()),
    );
    assert_eq!(
        hint("ip route-static vpn-instance BLUE 172.16.0.0 255.240.0.0 10.0.20.254"),
        Some("ip-route-static:BLUE:172.16.0.0:255.240.0.0".into()),
    );
}

#[test]
fn key_hint_vlan_batch_is_not_a_vlan() {
    assert_eq!(hint("vlan 10"), Some("vlan:10".into()));
    assert_eq!(hint("vlan batch 10 20 30"), None);
}

#[test]
fn key_hint_falls_back_to_the_shared_arms() {
    assert_eq!(
        hint("ntp server 10.0.0.53"),
        Some("ntp:server:10.0.0.53".into()),
    );
}

#[test]
fn key_hint_leaves_undo_lines_to_their_text() {
    assert_eq!(hint("undo portswitch"), None);
}

#[test]
fn key_hint_router_id_is_the_only_router_head_vrp_writes() {
    assert_eq!(hint("router id 10.255.255.1"), Some("router:id".into()));
}
