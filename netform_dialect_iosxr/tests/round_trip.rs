use netform_dialect_iosxr::parse_iosxr;
use netform_ir::{Node, TriviaKind};

const XR_CONFIG: &str = "\
!! IOS XR Configuration 7.3.2
!! Last configuration change at Tue Aug 12 09:31:22 2026 by netops
!
hostname xr-pe-01
!
vrf CUSTOMER-A
 address-family ipv4 unicast
  import route-target
   65001:100
  !
 !
!
interface Loopback0
 ipv4 address 10.255.255.1 255.255.255.255
!
interface MgmtEth0/RP0/CPU0/0
 ipv4 address 10.0.0.1 255.255.255.0
!
interface Bundle-Ether10
 description core-uplink
 ipv4 address 192.0.2.1 255.255.255.252
!
interface TenGigE0/0/0/0
 bundle id 10 mode active
!
prefix-set CUSTOMER-PFX
  10.0.0.0/8 le 24,
  192.0.2.0/24
end-set
!
as-path-set TRANSIT
  ios-regex '^65002_'
end-set
!
community-set NO-EXPORT-SET
  65001:666
end-set
!
extcommunity-set rt CUSTOMER-RT
  65001:100
end-set
!
rd-set CORE-RD
  10.255.255.1:0
end-set
!
route-policy CUSTOMER-IN
  if destination in CUSTOMER-PFX then
    set community NO-EXPORT-SET
    pass
  else
    drop
  endif
end-policy
!
route-policy PASS-ALL
  pass
end-policy
!
router static
 address-family ipv4 unicast
  0.0.0.0/0 10.0.0.254
 !
!
router bgp 65001
 bgp router-id 10.255.255.1
 neighbor-group CUSTOMER-V4
  remote-as 65002
  address-family ipv4 unicast
   route-policy CUSTOMER-IN in
  !
 !
!
end
";

#[test]
fn parse_iosxr_round_trips_a_full_configuration() {
    assert_eq!(parse_iosxr(XR_CONFIG).render(), XR_CONFIG);
}

#[test]
fn parse_iosxr_round_trips_crlf_and_a_missing_final_newline() {
    let cfg = "route-policy PASS-ALL\r\n  pass\r\nend-policy";
    assert_eq!(parse_iosxr(cfg).render(), cfg);
}

fn root_block<'a>(doc: &'a netform_ir::Document, header: &str) -> &'a netform_ir::BlockNode {
    doc.roots
        .iter()
        .find_map(|id| match doc.node(*id) {
            Some(Node::Block(block)) if block.header.raw == header => Some(block),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no root block headed {header}"))
}

#[test]
fn end_policy_becomes_the_route_policy_footer() {
    let doc = parse_iosxr(XR_CONFIG);
    let block = root_block(&doc, "route-policy CUSTOMER-IN");

    assert_eq!(
        block.footer.as_ref().map(|f| f.raw.as_str()),
        Some("end-policy"),
    );
    assert_eq!(
        block.header.key_hint.as_deref(),
        Some("route-policy:CUSTOMER-IN"),
    );
}

#[test]
fn end_set_becomes_the_footer_of_every_set_family() {
    let doc = parse_iosxr(XR_CONFIG);

    for header in [
        "prefix-set CUSTOMER-PFX",
        "as-path-set TRANSIT",
        "community-set NO-EXPORT-SET",
        "extcommunity-set rt CUSTOMER-RT",
        "rd-set CORE-RD",
    ] {
        let block = root_block(&doc, header);
        assert_eq!(
            block.footer.as_ref().map(|f| f.raw.as_str()),
            Some("end-set"),
            "{header} kept no footer",
        );
    }
}

#[test]
fn a_terminator_closing_no_block_stays_an_ordinary_line() {
    let cfg = "end-policy\nhostname xr-pe-01\n";
    let doc = parse_iosxr(cfg);

    assert_eq!(doc.render(), cfg);
    assert!(matches!(doc.node(doc.roots[0]), Some(Node::Line(_))));
}

#[test]
fn a_second_terminator_is_not_swallowed_by_an_already_footed_block() {
    let cfg = "route-policy PASS-ALL\n  pass\nend-policy\nend-policy\n";
    let doc = parse_iosxr(cfg);

    assert_eq!(doc.render(), cfg);
    assert_eq!(doc.roots.len(), 2);
}

#[test]
fn a_bare_end_is_not_a_terminator() {
    let doc = parse_iosxr(XR_CONFIG);
    let block = root_block(&doc, "router bgp 65001");

    assert!(block.footer.is_none());
    assert!(doc.roots.iter().any(|id| matches!(
        doc.node(*id),
        Some(Node::Line(line)) if line.raw == "end"
    )));
}

#[test]
fn banner_bodies_stay_literal() {
    let cfg = "\
banner motd ^
route-policy NOT-CONFIG
end-policy
^
hostname xr-pe-01
";
    let doc = parse_iosxr(cfg);

    let trivia: Vec<(&str, TriviaKind)> = doc
        .roots
        .iter()
        .map(|id| match doc.node(*id).expect("node in arena") {
            Node::Line(line) => (line.raw.as_str(), line.trivia),
            Node::Block(block) => (block.header.raw.as_str(), block.header.trivia),
        })
        .collect();

    assert_eq!(
        trivia,
        vec![
            ("banner motd ^", TriviaKind::Content),
            ("route-policy NOT-CONFIG", TriviaKind::Literal),
            ("end-policy", TriviaKind::Literal),
            ("^", TriviaKind::Literal),
            ("hostname xr-pe-01", TriviaKind::Content),
        ],
    );
    assert_eq!(doc.render(), cfg);
}

#[test]
fn comment_prefixes_cover_the_double_bang_header() {
    let doc = parse_iosxr(XR_CONFIG);
    let first = doc.node(doc.roots[0]).expect("node in arena");

    assert!(matches!(
        first,
        Node::Line(line) if line.trivia == TriviaKind::Comment
    ));
}

#[test]
fn qos_and_group_terminators_attach_when_they_close_a_block() {
    let cfg = "\
class-map match-any VOICE
  match precedence 5
end-class-map
policy-map PARENT-SHAPER
  class VOICE
    priority level 1
end-policy-map
group G-CORE-INTERFACE
  interface 'TenGigE.*'
    mtu 9216
end-group
";
    let doc = parse_iosxr(cfg);

    assert_eq!(doc.render(), cfg);
    for (header, footer) in [
        ("class-map match-any VOICE", "end-class-map"),
        ("policy-map PARENT-SHAPER", "end-policy-map"),
        ("group G-CORE-INTERFACE", "end-group"),
    ] {
        assert_eq!(
            root_block(&doc, header)
                .footer
                .as_ref()
                .map(|f| f.raw.as_str()),
            Some(footer),
        );
    }
}

#[test]
fn an_indented_terminator_stays_a_child_line() {
    let cfg = "\
class-map match-any VOICE
 match precedence 5
 end-class-map
!
";
    let doc = parse_iosxr(cfg);
    let block = root_block(&doc, "class-map match-any VOICE");

    assert_eq!(doc.render(), cfg);
    assert!(block.footer.is_none());
    assert_eq!(block.children.len(), 2);
}
