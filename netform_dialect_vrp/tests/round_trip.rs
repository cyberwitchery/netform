use netform_dialect_vrp::parse_vrp;
use netform_ir::{Node, TriviaKind};

const VRP_CONFIG: &str = "\
#
sysname CE-ACCESS-01
#
vlan batch 10 20 30
#
vlan 10
 description users
#
vlan 20
 description servers
#
ip vpn-instance BLUE
 ipv4-family
  route-distinguisher 65000:100
  vpn-target 65000:100 export-extcommunity
  vpn-target 65000:100 import-extcommunity
#
acl number 3000
 rule 5 permit ip source 10.0.10.0 0.0.0.255
 rule 10 deny ip
#
acl name MGMT-IN advance
 rule 5 permit tcp destination-port eq 22
#
traffic classifier CLASS-USERS operator or
 if-match acl 3000
#
traffic behavior BEHAVE-USERS
 remark dscp af31
#
traffic policy POLICY-EDGE
 classifier CLASS-USERS behavior BEHAVE-USERS
#
ip ip-prefix DEFAULT-ONLY index 10 permit 0.0.0.0 0
#
route-policy EXPORT-BLUE permit node 10
 if-match ip-prefix DEFAULT-ONLY
#
interface Vlanif10
 ip address 10.0.10.1 255.255.255.0
#
interface Eth-Trunk1
 port link-type trunk
 port trunk allow-pass vlan 10 20
#
interface GigabitEthernet0/0/1
 eth-trunk 1
#
interface GigabitEthernet0/0/2
 port link-type access
 port default vlan 10
#
interface GigabitEthernet0/0/3
 undo portswitch
 ip binding vpn-instance BLUE
 ip address 10.0.20.1 255.255.255.0
#
interface LoopBack0
 ip address 10.255.255.1 255.255.255.255
#
interface NULL0
#
bgp 65000
 router-id 10.255.255.1
 peer 10.0.0.2 as-number 65001
 peer 10.0.0.2 description core-a
 #
 ipv4-family unicast
  undo synchronization
  peer 10.0.0.2 enable
 #
 ipv4-family vpn-instance BLUE
  import-route direct
#
ospf 1 router-id 10.255.255.1
 area 0.0.0.0
  network 10.0.10.0 0.0.0.255
#
isis 1
 network-entity 49.0001.0102.5525.5001.00
#
ip route-static 0.0.0.0 0.0.0.0 10.0.0.254
ip route-static vpn-instance BLUE 172.16.0.0 255.240.0.0 10.0.20.254
#
local-user netops password irreversible-cipher %^%#secret%^%#
local-user netops service-type ssh
#
user-interface con 0
 authentication-mode password
#
user-interface vty 0 4
 authentication-mode aaa
 protocol inbound ssh
#
return
";

#[test]
fn parse_vrp_round_trips_a_full_configuration() {
    assert_eq!(parse_vrp(VRP_CONFIG).render(), VRP_CONFIG);
}

#[test]
fn parse_vrp_round_trips_crlf_and_a_missing_final_newline() {
    let cfg = "#\r\ninterface Vlanif10\r\n ip address 10.0.10.1 255.255.255.0";
    assert_eq!(parse_vrp(cfg).render(), cfg);
}

#[test]
fn a_hash_delimited_banner_round_trips_and_its_body_is_literal() {
    let cfg = "header login information #\nwelcome to CE-ACCESS-01\n#\nsysname CE-ACCESS-01\n";
    let doc = parse_vrp(cfg);

    assert_eq!(doc.render(), cfg);
    assert_eq!(
        root_trivia(&doc),
        vec![
            ("header login information #", TriviaKind::Content),
            ("welcome to CE-ACCESS-01", TriviaKind::Literal),
            ("#", TriviaKind::Literal),
            ("sysname CE-ACCESS-01", TriviaKind::Content),
        ],
    );
}

#[test]
fn header_login_information_body_lines_are_literal_not_configuration() {
    let doc = parse_vrp(
        "header login information %\nTicket queue:\n#12345 pending\nsysname NOT-A-COMMAND\n%\nsysname CE-1\n",
    );

    assert_eq!(
        root_trivia(&doc),
        vec![
            ("header login information %", TriviaKind::Content),
            ("Ticket queue:", TriviaKind::Literal),
            ("#12345 pending", TriviaKind::Literal),
            ("sysname NOT-A-COMMAND", TriviaKind::Literal),
            ("%", TriviaKind::Literal),
            ("sysname CE-1", TriviaKind::Content),
        ],
    );
}

#[test]
fn header_shell_information_opens_a_literal_body_too() {
    let doc = parse_vrp("header shell information $\n#in the body\n$\n");

    assert_eq!(
        root_trivia(&doc),
        vec![
            ("header shell information $", TriviaKind::Content),
            ("#in the body", TriviaKind::Literal),
            ("$", TriviaKind::Literal),
        ],
    );
}

#[test]
fn a_self_contained_header_opens_no_literal_body() {
    let doc = parse_vrp(
        "header login information \"Welcome\"\nsysname NOT-A-COMMAND\ndescription \"Welcome\"\n#\nsysname CE-1\n",
    );

    assert_eq!(
        root_trivia(&doc),
        vec![
            ("header login information \"Welcome\"", TriviaKind::Content),
            ("sysname NOT-A-COMMAND", TriviaKind::Content),
            ("description \"Welcome\"", TriviaKind::Content),
            ("#", TriviaKind::Comment),
            ("sysname CE-1", TriviaKind::Content),
        ],
    );
}

#[test]
fn header_login_file_references_a_file_and_opens_no_body() {
    let doc = parse_vrp(
        "header login file flash:/login.txt\nsysname NOT-A-COMMAND\nheader shell file flash:/shell.txt\n#\nsysname CE-1\n",
    );

    assert_eq!(
        root_trivia(&doc),
        vec![
            ("header login file flash:/login.txt", TriviaKind::Content),
            ("sysname NOT-A-COMMAND", TriviaKind::Content),
            ("header shell file flash:/shell.txt", TriviaKind::Content),
            ("#", TriviaKind::Comment),
            ("sysname CE-1", TriviaKind::Content),
        ],
    );
}

#[test]
fn vrp_does_not_recognize_the_ios_banner_spelling() {
    let doc = parse_vrp("banner motd ^C\nsysname NOT-A-COMMAND\n^C\n#\nsysname CE-1\n");

    assert_eq!(
        root_trivia(&doc),
        vec![
            ("banner motd ^C", TriviaKind::Content),
            ("sysname NOT-A-COMMAND", TriviaKind::Content),
            ("^C", TriviaKind::Content),
            ("#", TriviaKind::Comment),
            ("sysname CE-1", TriviaKind::Content),
        ],
    );
}

fn root_trivia(doc: &netform_ir::Document) -> Vec<(&str, TriviaKind)> {
    doc.roots
        .iter()
        .map(|id| match doc.node(*id).expect("node in arena") {
            Node::Line(line) => (line.raw.as_str(), line.trivia),
            Node::Block(block) => (block.header.raw.as_str(), block.header.trivia),
        })
        .collect()
}

#[test]
fn hash_separators_are_comments_not_content() {
    let doc = parse_vrp("#\nsysname CE-ACCESS-01\n#\n");

    assert_eq!(
        root_trivia(&doc),
        vec![
            ("#", TriviaKind::Comment),
            ("sysname CE-ACCESS-01", TriviaKind::Content),
            ("#", TriviaKind::Comment),
        ],
    );
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
fn indented_hash_separators_stay_inside_their_block() {
    let doc = parse_vrp(VRP_CONFIG);
    let bgp = root_block(&doc, "bgp 65000");

    let children: Vec<&str> = bgp
        .children
        .iter()
        .map(|id| match doc.node(*id).expect("node in arena") {
            Node::Line(line) => line.raw.trim(),
            Node::Block(block) => block.header.raw.trim(),
        })
        .collect();

    assert_eq!(
        children,
        vec![
            "router-id 10.255.255.1",
            "peer 10.0.0.2 as-number 65001",
            "peer 10.0.0.2 description core-a",
            "#",
            "ipv4-family unicast",
            "#",
            "ipv4-family vpn-instance BLUE",
        ],
    );
    assert_eq!(bgp.footer.as_ref().map(|f| f.raw.as_str()), None);
}

#[test]
fn vrp_blocks_have_no_footer_delimiter() {
    let doc = parse_vrp(VRP_CONFIG);

    for header in [
        "ip vpn-instance BLUE",
        "acl number 3000",
        "ospf 1 router-id 10.255.255.1",
    ] {
        assert_eq!(
            root_block(&doc, header)
                .footer
                .as_ref()
                .map(|f| f.raw.as_str()),
            None,
            "{header} grew a footer",
        );
    }
}
