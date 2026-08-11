//! score-based dialect auto-detection from configuration text.
//!
//! scans lines for dialect-specific patterns, accumulates per-dialect scores
//! (weights and acceptance thresholds are documented on the constants below),
//! and returns the highest-scoring dialect as a [`DialectHint`].  the winner
//! must both meet `MIN_CONFIDENCE_SCORE` and outscore the runner-up by
//! `MARGIN_FACTOR`; otherwise the result is [`DialectHint::Generic`].
//!
//! # Example
//!
//! ```rust
//! use netform_ir::detect::detect_dialect;
//! use netform_ir::DialectHint;
//!
//! let junos_cfg = "interfaces {\n    ge-0/0/0 {\n        mtu 9216;\n    }\n}\n";
//! assert_eq!(detect_dialect(junos_cfg), DialectHint::Named("junos".into()));
//!
//! assert_eq!(detect_dialect(""), DialectHint::Generic);
//! ```

use crate::DialectHint;

/// score for a highly distinctive, dialect-unique pattern (e.g. FortiOS
/// `config <section>`, NX-OS `feature <name>`, Junos top-level stanza names).
const STRONG_SIGNAL: i32 = 3;

/// score for a moderately distinctive pattern (e.g. FortiOS `end`/`next`,
/// Junos brace open/close, EOS non-slot and IOS XE speed-prefixed Ethernet
/// interfaces).
const MODERATE_SIGNAL: i32 = 2;

/// score for a pattern that weakly suggests a dialect (e.g. Junos trailing
/// semicolons, FortiOS plain `set <field>`, IOS XE wildcard masks in ACLs).
const WEAK_SIGNAL: i32 = 1;

/// minimum total score a dialect must reach to be considered detected (at
/// least one strong signal or multiple weaker ones).  below this threshold,
/// the input is too short or too ambiguous to identify.
const MIN_CONFIDENCE_SCORE: i32 = 3;

/// the winning dialect must outscore the runner-up by at least this factor.
/// a value of 2 means the winner needs ≥ 2× the runner-up's score.
const MARGIN_FACTOR: i32 = 2;

/// detect the likely network-device dialect from configuration text.
///
/// returns a [`DialectHint`] identifying the detected dialect:
/// - `DialectHint::Named("eos")` — Arista EOS
/// - `DialectHint::Named("fortios")` — Fortinet FortiOS
/// - `DialectHint::Named("iosxe")` — Cisco IOS XE
/// - `DialectHint::Named("junos")` — Juniper Junos
/// - `DialectHint::Named("nxos")` — Cisco NX-OS
/// - `DialectHint::Generic` — no dialect detected with sufficient confidence
pub fn detect_dialect(input: &str) -> DialectHint {
    let mut fortios: i32 = 0;
    let mut junos: i32 = 0;
    let mut nxos: i32 = 0;
    let mut eos: i32 = 0;
    let mut iosxe: i32 = 0;

    for raw_line in input.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line == "!" {
            continue;
        }

        // `config <section>` / `end` block structure is unique to FortiOS.
        if line.starts_with("config ")
            && !line.contains('{')
            && line.split_whitespace().count() >= 2
        {
            fortios += STRONG_SIGNAL;
        }
        if line.starts_with("edit ") {
            fortios += STRONG_SIGNAL;
        }
        if line == "end" {
            fortios += MODERATE_SIGNAL;
        }
        if line == "next" {
            fortios += MODERATE_SIGNAL;
        }
        if line.starts_with("set ") || line.starts_with("unset ") {
            let second = line.split_whitespace().nth(1).unwrap_or("");
            if is_junos_stanza_name(second) {
                // `set interfaces ...`, `set protocols ...` etc — Junos set-style.
                junos += STRONG_SIGNAL;
            } else {
                // plain `set <field> <value>` leans FortiOS (inside config blocks).
                fortios += WEAK_SIGNAL;
            }
        }

        // brace-and-semicolon syntax is highly distinctive.
        if line.ends_with('{') {
            junos += MODERATE_SIGNAL;
        }
        if line == "}" || line.ends_with("};") {
            junos += MODERATE_SIGNAL;
        }
        if line.ends_with(';') && !line.ends_with("};") {
            junos += WEAK_SIGNAL;
        }
        // junos-specific stanza names at top-level (hierarchical style).
        if is_junos_stanza_name(line.split_whitespace().next().unwrap_or("")) {
            junos += STRONG_SIGNAL;
        }

        // `feature <name>` is unique to NX-OS among supported dialects.
        if line.starts_with("feature ") {
            nxos += STRONG_SIGNAL;
        }
        // slot/port interfaces: Ethernet1/1, port-channel1, etc.
        if line.starts_with("interface ") {
            let iface = line.trim_start_matches("interface ");
            if is_iosxe_ethernet_name(iface) {
                iosxe += MODERATE_SIGNAL;
            } else if iface.starts_with("Ethernet") && iface.contains('/') {
                // NX-OS uses plain Ethernet with slot/port (Ethernet1/1).
                nxos += STRONG_SIGNAL;
            } else if iface.starts_with("Ethernet") || iface.starts_with("Management") {
                // no slot → could be EOS.
                eos += MODERATE_SIGNAL;
            }
        }
        if line.starts_with("vpc ") {
            nxos += STRONG_SIGNAL;
        }
        if line.starts_with("role name ") {
            nxos += MODERATE_SIGNAL;
        }

        // `ip access-list extended` is a strong IOS XE marker.
        if line.starts_with("ip access-list extended ") {
            iosxe += STRONG_SIGNAL;
        }
        // `ip address` masks: dotted → IOS XE, CIDR → EOS. tokenize once, test both.
        if line.starts_with("ip address ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 && looks_like_dotted_mask(parts[3]) {
                iosxe += MODERATE_SIGNAL;
            }
            if parts.len() >= 3 && parts[2].contains('/') {
                eos += MODERATE_SIGNAL;
            }
        }
        // `network ... mask ...` in BGP address-family.
        if line.contains(" mask ") && line.starts_with("network ") {
            iosxe += MODERATE_SIGNAL;
        }
        // wildcard masks in ACL permit/deny lines.
        if (line.starts_with("permit ") || line.starts_with("deny "))
            && line.split_whitespace().any(looks_like_dotted_mask)
        {
            iosxe += WEAK_SIGNAL;
        }

        if line.starts_with("ip access-list ") && !line.contains("extended") {
            eos += MODERATE_SIGNAL;
        }
        // numbered ACL entries (EOS style: `10 permit ...`).
        if let Some(first) = line.split_whitespace().next()
            && first.parse::<u32>().is_ok()
            && (line.contains(" permit ") || line.contains(" deny "))
        {
            eos += WEAK_SIGNAL;
        }
    }

    let candidates = [
        ("fortios", fortios),
        ("junos", junos),
        ("nxos", nxos),
        ("eos", eos),
        ("iosxe", iosxe),
    ];

    let mut sorted = candidates;
    sorted.sort_by_key(|c| std::cmp::Reverse(c.1));

    let (best_name, best_score) = sorted[0];
    let (_, second_score) = sorted[1];

    if best_score < MIN_CONFIDENCE_SCORE {
        return DialectHint::Generic;
    }
    if best_score < second_score * MARGIN_FACTOR {
        return DialectHint::Generic;
    }

    DialectHint::Named(best_name.to_string())
}

/// returns `true` if `name` is a well-known Junos top-level stanza name.
fn is_junos_stanza_name(name: &str) -> bool {
    matches!(
        name,
        "interfaces"
            | "protocols"
            | "policy-options"
            | "routing-options"
            | "forwarding-options"
            | "class-of-service"
            | "system"
            | "security"
            | "firewall"
            | "vlans"
            | "chassis"
            | "snmp"
            | "applications"
            | "groups"
            | "routing-instances"
    )
}

/// returns `true` if `name` is an IOS XE speed-prefixed Ethernet interface
/// name (e.g. `GigabitEthernet1/0/1`, `HundredGigE1/0/1`).
fn is_iosxe_ethernet_name(name: &str) -> bool {
    const PREFIXES: [&str; 9] = [
        "AppGigabitEthernet",
        "FastEthernet",
        "FiveGigabitEthernet",
        "FortyGigabitEthernet",
        "GigabitEthernet",
        "HundredGigE",
        "TenGigabitEthernet",
        "TwentyFiveGigE",
        "TwoGigabitEthernet",
    ];
    PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// returns `true` if `s` looks like a dotted-decimal subnet or wildcard mask
/// (e.g. `255.255.255.0` or `0.0.0.255`).
fn looks_like_dotted_mask(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
