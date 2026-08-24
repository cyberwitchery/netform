//! `--dialect auto` over the registry: every vendor's signals, the scoring
//! thresholds and the margin rule, exercised end to end.

use netform_dialects::detect_dialect;
use netform_ir::DialectHint;

#[test]
fn detect_fortios() {
    let input = "\
config system global
    set hostname \"FortiGate-01\"
    set timezone 04
end
config firewall address
    edit \"web-server\"
        set type ipmask
        set subnet 10.0.1.10 255.255.255.255
    next
end
";
    assert_eq!(detect_dialect(input), DialectHint::Named("fortios".into()));
}

#[test]
fn detect_junos_hierarchical() {
    let input = "\
interfaces {
    ge-0/0/0 {
        description \"uplink\";
        mtu 9216;
        unit 0 {
            family inet {
                address 192.0.2.2/30;
            }
        }
    }
}
";
    assert_eq!(detect_dialect(input), DialectHint::Named("junos".into()));
}

#[test]
fn detect_junos_set_style() {
    let input = "\
set interfaces ge-0/0/0 description \"uplink\"
set interfaces ge-0/0/0 mtu 9216
set interfaces ge-0/0/0 unit 0 family inet address 192.0.2.2/30
set protocols bgp group EBGP type external
set routing-options static route 0.0.0.0/0 next-hop 10.0.0.1
";
    assert_eq!(detect_dialect(input), DialectHint::Named("junos".into()));
}

#[test]
fn detect_nxos() {
    let input = "\
hostname n9k-leaf-01
!
feature bgp
feature interface-vlan
feature lacp
!
vlan 10
  name SERVERS
!
interface Ethernet1/1
  description uplink-spine-a
  mtu 9216
  ip address 192.0.2.2/31
  no shutdown
!
router bgp 65001
  router-id 10.255.255.1
";
    assert_eq!(detect_dialect(input), DialectHint::Named("nxos".into()));
}

#[test]
fn detect_eos() {
    let input = "\
hostname leaf-01
interface Ethernet1
   description uplink-spine-a
   mtu 9214
   ip address 192.0.2.2/31
   no shutdown
router bgp 65000
   router-id 10.255.255.1
ip access-list ACL-EDGE-IN
   10 permit tcp 10.10.1.0/24 any eq https
   20 permit tcp 10.10.1.0/24 any eq ssh
   90 deny ip any any log
";
    assert_eq!(detect_dialect(input), DialectHint::Named("eos".into()));
}

#[test]
fn detect_iosxe() {
    let input = "\
interface GigabitEthernet0/0/0
  description uplink-core-a
  mtu 9216
  ip address 192.0.2.2 255.255.255.252
  no shutdown
router bgp 65000
  bgp log-neighbor-changes
  address-family ipv4 unicast
    network 10.10.1.0 mask 255.255.255.0
ip access-list extended ACL-EDGE-IN
  permit tcp 10.10.1.0 0.0.0.255 any eq 443
  deny ip any any log
";
    assert_eq!(detect_dialect(input), DialectHint::Named("iosxe".into()));
}

#[test]
fn detect_iosxe_layer2_only_config() {
    let input = "\
hostname cat9k-access-01
!
interface GigabitEthernet1/0/1
 switchport mode access
 switchport access vlan 10
!
interface GigabitEthernet1/0/2
 switchport mode access
 switchport access vlan 20
!
";
    assert_eq!(detect_dialect(input), DialectHint::Named("iosxe".into()));
}

#[test]
fn detect_iosxe_from_other_speed_prefixes() {
    let input = "\
interface TenGigabitEthernet1/1/1
interface HundredGigE1/0/1
";
    assert_eq!(detect_dialect(input), DialectHint::Named("iosxe".into()));
}

#[test]
fn detect_generic_when_iosxe_and_nxos_interfaces_mix() {
    // iosxe MODERATE(2) vs nxos STRONG(3): 3 < 2 * MARGIN_FACTOR(2).
    let input = "\
interface GigabitEthernet1/0/1
interface Ethernet1/1
";
    assert_eq!(detect_dialect(input), DialectHint::Generic);
}

#[test]
fn detect_generic_for_empty() {
    assert_eq!(detect_dialect(""), DialectHint::Generic);
}

#[test]
fn detect_generic_for_plain_text() {
    let input = "\
hostname router
# a comment
some random config line
";
    assert_eq!(detect_dialect(input), DialectHint::Generic);
}

#[test]
fn detect_generic_when_ambiguous() {
    // minimal content with weak signals from multiple dialects.
    let input = "\
set hostname myrouter
interface Ethernet1
";
    // both junos/fortios and eos get mild scores — should fall back.
    assert_eq!(detect_dialect(input), DialectHint::Generic);
}

#[test]
fn detect_generic_on_exact_tie() {
    // craft input where NX-OS and EOS each score exactly 3.
    // `feature ospf` → nxos += 3
    // `ip access-list ACL-IN` → eos += 2
    // `10 permit tcp any any` → eos += 1  (numbered ACL entry)
    let input = "\
feature ospf
ip access-list ACL-IN
   10 permit tcp any any
";
    assert_eq!(detect_dialect(input), DialectHint::Generic);
}

#[test]
fn detect_at_minimum_score_single_strong_signal() {
    // one STRONG_SIGNAL (3) with no competition → exactly MIN_CONFIDENCE_SCORE.
    let input = "feature ospf\n";
    assert_eq!(detect_dialect(input), DialectHint::Named("nxos".into()));
}

#[test]
fn detect_below_minimum_score_single_moderate_signal() {
    // one MODERATE_SIGNAL (2) → below MIN_CONFIDENCE_SCORE → Generic.
    let input = "role name admin\n";
    assert_eq!(detect_dialect(input), DialectHint::Generic);
}

#[test]
fn detect_margin_exact_boundary_passes() {
    // iosxe = STRONG(3) + WEAK(1) = 4, eos = MODERATE(2).
    // 4 >= 2 * MARGIN_FACTOR(2) → passes margin check.
    let input = "\
ip access-list extended ACL-IN
  permit tcp any 0.0.0.255 any
interface Ethernet1
";
    assert_eq!(detect_dialect(input), DialectHint::Named("iosxe".into()));
}

#[test]
fn detect_margin_just_below_boundary_fails() {
    // iosxe = STRONG(3), eos = MODERATE(2).
    // 3 < 2 * MARGIN_FACTOR(2) = 4 → fails margin check → Generic.
    let input = "\
ip access-list extended ACL-IN
interface Ethernet1
";
    assert_eq!(detect_dialect(input), DialectHint::Generic);
}

#[test]
fn detect_clear_winner_no_runner_up() {
    // two STRONG_SIGNAL NX-OS features, everything else at zero.
    // nxos = 6, second = 0 → 6 >= 0 → clear win.
    let input = "\
feature bgp
feature ospf
";
    assert_eq!(detect_dialect(input), DialectHint::Named("nxos".into()));
}

#[test]
fn detect_strong_signal_drowned_by_cross_dialect_noise() {
    // NX-OS gets one strong signal, but Junos accumulates more from
    // brace/semicolon syntax surrounding it.
    // nxos = STRONG(3)
    // junos = STRONG(3) [interfaces stanza] + MODERATE(2) [open brace]
    //       + WEAK(1) [semicolon] + MODERATE(2) [close brace] = 8
    // junos 8 >= 3*2 → junos wins.
    let input = "\
feature ospf
interfaces {
    mtu 9216;
}
";
    assert_eq!(detect_dialect(input), DialectHint::Named("junos".into()));
}

#[test]
fn detect_two_moderate_signals_reach_margin() {
    // two MODERATE_SIGNAL FortiOS lines: end(2) + next(2) = 4.
    // 4 >= MIN_CONFIDENCE_SCORE(3) ✓, second = 0, 4 >= 0 ✓ → detected.
    let input = "\
end
next
";
    assert_eq!(detect_dialect(input), DialectHint::Named("fortios".into()));
}

#[test]
fn detect_only_weak_signals_below_threshold() {
    // two WEAK_SIGNAL lines: junos semicolons.
    // junos = 1 + 1 = 2 → below MIN_CONFIDENCE_SCORE → Generic.
    let input = "\
mtu 9216;
description uplink;
";
    assert_eq!(detect_dialect(input), DialectHint::Generic);
}

#[test]
fn detect_three_weak_signals_reach_threshold() {
    // three WEAK_SIGNAL junos semicolons = 3 → exactly MIN_CONFIDENCE_SCORE.
    // no competition → detected.
    let input = "\
mtu 9216;
description uplink;
no-readvertise;
";
    assert_eq!(detect_dialect(input), DialectHint::Named("junos".into()));
}

const IOSXR: &str = "\
hostname xr-pe-01
!
interface Bundle-Ether10
 ipv4 address 192.0.2.1 255.255.255.252
!
interface TenGigE0/0/0/0
 bundle id 10 mode active
!
prefix-set CUSTOMER-PFX
  10.0.0.0/8 le 24
end-set
!
route-policy CUSTOMER-IN
  if destination in CUSTOMER-PFX then
    pass
  endif
end-policy
!
router bgp 65001
 neighbor-group CUSTOMER-V4
  remote-as 65002
!
end
";

#[test]
fn detect_iosxr() {
    assert_eq!(detect_dialect(IOSXR), DialectHint::Named("iosxr".into()));
}

#[test]
fn detect_iosxr_from_routing_policy_language_alone() {
    let input = "\
route-policy PASS-ALL
  pass
end-policy
";
    assert_eq!(detect_dialect(input), DialectHint::Named("iosxr".into()));
}

#[test]
fn detect_iosxr_from_four_part_slot_interfaces() {
    let input = "\
interface TenGigE0/0/0/0
interface FortyGigE0/0/0/1.100
";
    assert_eq!(detect_dialect(input), DialectHint::Named("iosxr".into()));
}

#[test]
fn detect_iosxe_keeps_its_four_part_breakout_subports() {
    let input = "\
interface TwentyFiveGigE1/0/20/1
interface HundredGigE1/0/21/1
interface FortyGigabitEthernet1/0/22/1
";
    assert_eq!(detect_dialect(input), DialectHint::Named("iosxe".into()));
}

#[test]
fn detect_iosxr_from_xr_only_interface_families() {
    let input = "\
interface MgmtEth0/RP0/CPU0/0
interface tunnel-ip1
interface BVI100
";
    assert_eq!(detect_dialect(input), DialectHint::Named("iosxr".into()));
}

#[test]
fn detect_iosxe_keeps_its_three_part_slot_interfaces() {
    let input = "\
interface GigabitEthernet0/0/0
interface HundredGigE1/0/1
interface TenGigabitEthernet1/1/1
";
    assert_eq!(detect_dialect(input), DialectHint::Named("iosxe".into()));
}

#[test]
fn detect_iosxr_ignores_four_part_names_other_dialects_spell() {
    for name in [
        "appgigabitethernet1/0/20/1",
        "BDI1/2/3/4",
        "fabric1/2/3/4",
        "fastethernet0/1/2/3",
        "fivegigabitethernet1/0/20/1",
        "fortygigabitethernet1/0/20/1",
        "FourHundredGig1/0/31/1",
        "gigabitethernet0/0/0/0",
        "HundredGigabitEthernet1/0/2/1",
        "hundredgige1/0/20/1",
        "Loopback1/2/3/4",
        "Management1/2/3/4",
        "mgmt1/2/3/4",
        "nve1/2/3/4",
        "Port-channel1/2/3/4",
        "Serial0/1/2/3",
        "tengigabitethernet1/0/20/1",
        "Tunnel1/2/3/4",
        "twentyfivegige1/0/20/1",
        "twogigabitethernet1/0/20/1",
        "TwoHundredGigE1/0/5/1",
        "Vlan1/2/3/4",
        "Vxlan1/2/3/4",
    ] {
        let input = format!("interface {name}\n");
        assert_eq!(
            detect_dialect(&input),
            DialectHint::Generic,
            "{name} scores a dialect on slot shape alone",
        );
    }
}

#[test]
fn detect_iosxe_keeps_c9500_32c_breakout_subports() {
    for subports in [3, 4, 8, 12, 24] {
        let mut input = String::from(C9500_32C_FIXTURE);

        for index in 0..subports {
            let port = 2 + index / 4;
            let subport = 1 + index % 4;
            input.push_str(&format!(
                "interface HundredGigabitEthernet1/0/{port}/{subport}\n switchport mode access\n"
            ));
        }

        assert_eq!(
            detect_dialect(&input),
            DialectHint::Named("iosxe".into()),
            "{subports} breakout subports move the Catalyst off iosxe",
        );
    }
}

#[test]
fn detect_iosxr_reads_its_own_families_in_either_case() {
    for name in [
        "Bundle-Ether1",
        "bundle-ether1",
        "BVI1",
        "bvi1",
        "MgmtEth0/RP0/CPU0/0",
        "mgmteth0/RP0/CPU0/0",
        "PW-Ether1",
        "pw-ether1",
        "Tunnel-IP0/0/0/0",
        "tunnel-ip0/0/0/0",
    ] {
        let input = format!("interface {name}\n");
        assert_eq!(
            detect_dialect(&input),
            DialectHint::Named("iosxr".into()),
            "{name} loses IOS XR detection to its case",
        );
    }
}

#[test]
fn detect_nxos_keeps_its_ethernet_names_however_deep() {
    let input = "\
interface Ethernet1/2/3/4
interface Ethernet1/2/3/5
";
    assert_eq!(detect_dialect(input), DialectHint::Named("nxos".into()));
}

#[test]
fn iosxr_signals_leave_the_other_dialects_where_they_were() {
    for (input, expected) in [
        (IOSXE_FIXTURE, "iosxe"),
        (NXOS_FIXTURE, "nxos"),
        (EOS_FIXTURE, "eos"),
        (JUNOS_FIXTURE, "junos"),
        (FORTIOS_FIXTURE, "fortios"),
    ] {
        assert_eq!(
            detect_dialect(input),
            DialectHint::Named(expected.into()),
            "{expected} fixture no longer detects as itself",
        );
    }
}

const C9500_32C_FIXTURE: &str = "\
hostname c9500-32c-lab
vrf definition MGMT
 address-family ipv4
 exit-address-family
vlan 10
 name users
vlan 20
 name servers
ip access-list extended BLOCK-RFC1918
 deny   ip 10.0.0.0 0.255.255.255 any
 permit ip any any
ip access-list extended MGMT-IN
 permit tcp any any eq 22
interface GigabitEthernet0/0
 vrf forwarding MGMT
 ip address 10.0.0.5 255.255.255.0
interface TenGigabitEthernet1/0/47
 switchport mode trunk
router bgp 65001
 bgp log-neighbor-changes
 network 192.0.2.0 mask 255.255.255.0
 neighbor 10.0.0.1 remote-as 65002
";

const IOSXE_FIXTURE: &str = "\
interface GigabitEthernet0/0/0
  description uplink-core-a
  ip address 192.0.2.2 255.255.255.252
interface HundredGigE1/0/20/1
  description breakout-leaf-1
interface HundredGigE1/0/20/2
  description breakout-leaf-2
interface HundredGigE1/0/20/3
  description breakout-leaf-3
router bgp 65000
  address-family ipv4 unicast
    network 10.10.1.0 mask 255.255.255.0
ip access-list extended ACL-EDGE-IN
  permit tcp 10.10.1.0 0.0.0.255 any eq 443
";

const NXOS_FIXTURE: &str = "\
feature bgp
feature interface-vlan
vlan 10
  name SERVERS
interface Ethernet1/1
  ip address 192.0.2.2/31
vpc domain 10
";

const EOS_FIXTURE: &str = "\
interface Ethernet1
   ip address 192.0.2.2/31
interface Management1
ip access-list ACL-EDGE-IN
   10 permit tcp 10.10.1.0/24 any eq https
";

const JUNOS_FIXTURE: &str = "\
interfaces {
    ge-0/0/0 {
        mtu 9216;
    }
}
";

const FORTIOS_FIXTURE: &str = "\
config system global
    set hostname \"FortiGate-01\"
end
config firewall address
    edit \"web-server\"
        set type ipmask
    next
end
";

#[test]
fn detect_skips_bang_commented_junos_stanza() {
    let input = "\
version 17.9
hostname sw1
!
! migrated from mx480, original Junos stanza kept for reference:
! system {
!     host-name sw1;
!     services {
!         ssh;
!     }
! }
!
interface GigabitEthernet1/0/1
 ip address 192.0.2.1 255.255.255.0
!
";
    assert_eq!(detect_dialect(input), DialectHint::Named("iosxe".into()));
}

#[test]
fn detect_skips_hash_commented_junos_stanza() {
    let input = "\
#config-version=FGT60F-7.2.5-FW-build1517-230606:opmode=0:vdom=0
# migrated from an SRX, previous stanza kept for reference:
# security {
#     policies {
#         from-zone trust to-zone untrust {
#             policy allow-web;
#         }
#     }
# }
config system global
    set hostname \"FGT60F\"
end
";
    assert_eq!(detect_dialect(input), DialectHint::Named("fortios".into()));
}

#[test]
fn detect_skips_undelimited_banner_body() {
    let input = "\
hostname sw1
!
banner motd
This system is for authorized use only;
Users have no expectation of privacy;
All activity may be monitored, recorded and audited;
Violators will be prosecuted to the full extent of the law;
EOF
!
interface Ethernet1
   ip address 10.0.0.1/31
";
    assert_eq!(detect_dialect(input), DialectHint::Named("eos".into()));
}

#[test]
fn detect_skips_delimited_banner_body() {
    let input = "\
hostname sw1
!
banner motd ^C
This system is for authorized use only;
Users have no expectation of privacy;
All activity may be monitored and recorded;
^C
!
interface GigabitEthernet1/0/1
 ip address 192.0.2.1 255.255.255.0
";
    assert_eq!(detect_dialect(input), DialectHint::Named("iosxe".into()));
}

#[test]
fn detect_scores_past_an_unterminated_banner() {
    let input = "\
hostname sw1
banner motd ^C
This system is for authorized use only;
interface GigabitEthernet1/0/1
 ip address 192.0.2.1 255.255.255.0
";
    assert_eq!(detect_dialect(input), DialectHint::Named("iosxe".into()));
}

#[test]
fn detect_scores_below_a_banner_whose_body_mentions_banner() {
    let input = "\
hostname sw1
!
banner motd ^C
WARNING: unauthorized access prohibited.
banner set by netops on 2026-01-01
^C
!
interface GigabitEthernet1/0/1
 ip address 192.0.2.1 255.255.255.0
 standby 1 ip 192.0.2.254
";
    assert_eq!(detect_dialect(input), DialectHint::Named("iosxe".into()));
}

#[test]
fn detect_scores_below_an_undelimited_banner_whose_body_mentions_banner() {
    let input = "\
hostname leaf1
!
banner motd
WARNING: unauthorized access prohibited.
banner reviewed by legal
EOF
!
interface Ethernet1
   ip address 192.0.2.1/24
!
ip access-list standby-acl
   10 permit ip any any
";
    assert_eq!(detect_dialect(input), DialectHint::Named("eos".into()));
}
