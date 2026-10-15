use netform_dialects::iosxr::parse;
use netform_ir::{DialectHint, Node};

fn hint(line: &str) -> Option<String> {
    let doc = parse(&format!("{line}\n"));
    match doc.node(doc.roots[0]).expect("node in arena") {
        Node::Line(l) => l.key_hint.clone(),
        Node::Block(b) => b.header.key_hint.clone(),
    }
}

#[test]
fn parse_iosxr_sets_named_dialect_hint() {
    let doc = parse("hostname xr-pe-01\n");
    assert_eq!(
        doc.metadata.dialect_hint,
        DialectHint::Named("iosxr".into())
    );
}

#[test]
fn key_hints() {
    for (line, expected) in [
        // interfaces: four-part slots, subinterfaces, speed variants.
        (
            "interface GigabitEthernet0/0/0/0",
            "interface:gigabitethernet:0/0/0/0",
        ),
        (
            "interface GigabitEthernet0/0/0/0.100",
            "interface:gigabitethernet:0/0/0/0.100",
        ),
        ("interface TenGigE0/0/0/1", "interface:tengige:0/0/0/1"),
        ("interface FortyGigE0/0/0/2", "interface:fortygige:0/0/0/2"),
        (
            "interface HundredGigE0/0/0/3",
            "interface:hundredgige:0/0/0/3",
        ),
        // XR-specific types.
        ("interface Bundle-Ether10", "interface:bundle-ether:10"),
        (
            "interface MgmtEth0/RP0/CPU0/0",
            "interface:mgmteth:0/RP0/CPU0/0",
        ),
        ("interface tunnel-ip1", "interface:tunnel-ip:1"),
        ("interface BVI100", "interface:bvi:100"),
        ("interface PW-Ether5", "interface:pw-ether:5"),
        ("interface Loopback0", "interface:loopback:0"),
        ("interface nve1", "interface:nve:1"),
        // the type is case-insensitive.
        ("interface bundle-ether10", "interface:bundle-ether:10"),
        ("interface TENGIGE0/0/0/1", "interface:tengige:0/0/0/1"),
        // an unknown type falls back to the raw name.
        ("interface Ethernet1/1", "interface:Ethernet1/1"),
        // vrf uses the bare form.
        ("vrf CUSTOMER-A", "vrf:CUSTOMER-A"),
        ("router bgp 65001", "router:bgp:65001"),
        ("router ospf CORE", "router:ospf:CORE"),
        ("router isis 1", "router:isis:1"),
        ("router static", "router:static"),
        ("route-policy CUSTOMER-IN", "route-policy:CUSTOMER-IN"),
        // RPL set families.
        ("prefix-set CUSTOMER-PFX", "prefix-set:CUSTOMER-PFX"),
        ("as-path-set TRANSIT", "as-path-set:TRANSIT"),
        ("community-set NO-EXPORT-SET", "community-set:NO-EXPORT-SET"),
        (
            "extcommunity-set rt CUSTOMER-RT",
            "extcommunity-set:rt:CUSTOMER-RT",
        ),
        ("rd-set CORE-RD", "rd-set:CORE-RD"),
        // BGP group templates.
        ("neighbor-group CUSTOMER-V4", "neighbor-group:CUSTOMER-V4"),
        ("af-group TRANSIT-V4", "af-group:TRANSIT-V4"),
        ("session-group PEER-COMMON", "session-group:PEER-COMMON"),
        (
            "ipv4 access-list ACL-EDGE-IN",
            "ipv4-access-list:ACL-EDGE-IN",
        ),
        // falls back to the shared arms.
        ("ipv6 access-list ACL6-IN", "ipv6-access-list:ACL6-IN"),
        ("line template MGMT", "line:template:MGMT"),
        ("ntp server 10.0.0.1", "ntp:server:10.0.0.1"),
        ("policy-map PARENT-SHAPER", "policy-map:PARENT-SHAPER"),
        // typed class-maps and policy-maps (0.10.1).
        (
            "class-map type traffic match-any CM-TRAFFIC",
            "class-map:traffic:CM-TRAFFIC",
        ),
        (
            "class-map type qos match-any CM-QOS",
            "class-map:qos:CM-QOS",
        ),
        ("policy-map type pbr PM-PBR", "policy-map:pbr:PM-PBR"),
    ] {
        assert_eq!(hint(line).as_deref(), Some(expected), "{line}");
    }
}

#[test]
fn unkeyed_lines_have_no_hint() {
    for line in [
        "interface",
        // a bare set or group head.
        "route-policy",
        "prefix-set",
        "as-path-set",
        "community-set",
        "extcommunity-set",
        "rd-set",
        "neighbor-group",
        "af-group",
        "session-group",
        "ipv4 address 192.0.2.1 255.255.255.252",
        "hostname xr-pe-01",
        "commit",
    ] {
        assert_eq!(hint(line), None, "{line} should not key");
    }
}

#[test]
fn comments_and_blank_lines_are_never_keyed() {
    let doc = parse("!! IOS XR Configuration 7.3.2\n!\n\n");
    for id in &doc.roots {
        let Node::Line(line) = doc.node(*id).expect("node in arena") else {
            panic!("comment or blank should not open a block");
        };
        assert_eq!(line.key_hint, None);
    }
}
