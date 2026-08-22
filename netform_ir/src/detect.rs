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

use crate::{DialectHint, ios_like_literal_region};

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
/// - `DialectHint::Named("iosxr")` — Cisco IOS XR
/// - `DialectHint::Named("junos")` — Juniper Junos
/// - `DialectHint::Named("nxos")` — Cisco NX-OS
/// - `DialectHint::Named("vrp")` — Huawei VRP
/// - `DialectHint::Generic` — no dialect detected with sufficient confidence
pub fn detect_dialect(input: &str) -> DialectHint {
    let mut fortios: i32 = 0;
    let mut junos: i32 = 0;
    let mut nxos: i32 = 0;
    let mut eos: i32 = 0;
    let mut iosxe: i32 = 0;
    let mut iosxr: i32 = 0;
    let mut vrp: i32 = 0;

    let lines: Vec<&str> = input.lines().map(str::trim).collect();

    for (&line, scorable) in lines.iter().zip(scorable_lines(&lines)) {
        if !scorable {
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
            } else if is_iosxr_interface_name(iface) {
                iosxr += STRONG_SIGNAL;
            } else if iface.starts_with("Ethernet") && iface.contains('/') {
                // NX-OS uses plain Ethernet with slot/port (Ethernet1/1).
                nxos += STRONG_SIGNAL;
            } else if iface.starts_with("Ethernet") || iface.starts_with("Management") {
                // no slot → could be EOS.
                eos += MODERATE_SIGNAL;
            } else if is_vrp_interface_name(iface) {
                vrp += STRONG_SIGNAL;
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

        // Routing Policy Language and its `*-set` families are XR-only.
        if line.starts_with("route-policy ")
            || line.starts_with("prefix-set ")
            || line.starts_with("as-path-set ")
            || line.starts_with("community-set ")
            || line.starts_with("extcommunity-set ")
            || line.starts_with("rd-set ")
        {
            iosxr += STRONG_SIGNAL;
        }
        if line == "end-policy" || line == "end-set" {
            iosxr += MODERATE_SIGNAL;
        }
        // XR templates BGP peers instead of repeating the settings per neighbor.
        if line.starts_with("neighbor-group ")
            || line.starts_with("af-group ")
            || line.starts_with("session-group ")
        {
            iosxr += STRONG_SIGNAL;
        }
        // XR addresses interfaces under `ipv4`, the rest of the family under `ip`.
        if line.starts_with("ipv4 address ") || line.starts_with("ipv4 access-list ") {
            iosxr += MODERATE_SIGNAL;
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

        vrp += vrp_signal_score(line);
    }

    let candidates = [
        ("fortios", fortios),
        ("junos", junos),
        ("nxos", nxos),
        ("eos", eos),
        ("iosxe", iosxe),
        ("iosxr", iosxr),
        ("vrp", vrp),
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

/// marks the lines that carry configuration syntax; blank lines, comments and
/// banner bodies are excluded.  banners are walked with a cursor, as the parser
/// walks them, so a line inside a body never opens a banner of its own.
fn scorable_lines(lines: &[&str]) -> Vec<bool> {
    let mut scorable: Vec<bool> = lines
        .iter()
        .map(|line| !line.is_empty() && !is_comment(line))
        .collect();

    let mut idx = 0usize;
    while idx < lines.len() {
        match banner_body_end(lines, idx) {
            Some(end) => {
                scorable[idx + 1..=end].fill(false);
                idx = end + 1;
            }
            None => idx += 1,
        }
    }

    scorable
}

/// returns `true` if `line` opens a comment in any supported dialect: `!` in
/// the IOS family, `#` in Junos and FortiOS.
fn is_comment(line: &str) -> bool {
    line.starts_with('!') || line.starts_with('#')
}

/// returns the index of the line closing the banner opened at `idx`, so its
/// body spans `idx + 1 ..= end`.  `None` when `idx` opens no banner or the
/// delimiter never reappears.
fn banner_body_end(lines: &[&str], idx: usize) -> Option<usize> {
    let terminator = ios_like_literal_region(lines[idx])?;

    lines[idx + 1..]
        .iter()
        .position(|line| terminator.terminates(line))
        .map(|offset| idx + 1 + offset)
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

/// returns `true` if `name` is a Cisco IOS XR interface name: an XR-only
/// family, or a name in XR's four-part rack/slot/instance/port notation whose
/// interface type no other supported dialect spells.
fn is_iosxr_interface_name(name: &str) -> bool {
    const XR_ONLY_PREFIXES: [&str; 5] = ["bundle-ether", "bvi", "mgmteth", "pw-ether", "tunnel-ip"];

    let lower = name.to_ascii_lowercase();

    if XR_ONLY_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return true;
    }

    has_four_part_slot(name) && !is_other_dialect_interface_name(&lower)
}

/// interface type prefixes that a supported dialect other than IOS XR spells.
///
/// a superset of the IOS XE, NX-OS and EOS `*_INTERFACE_TYPES` tables: it also
/// carries vendor spellings those tables do not yet parse
/// (`hundredgigabitethernet`, `fourhundredgig`, `twohundredgig`), because a
/// name IOS XE spells must not score IOS XR whether or not netform can key it.
///
/// the containment is an invariant, not a coincidence — every entry of every
/// dialect table must start with one of these prefixes, or that dialect's
/// four-part interface names silently begin scoring IOS XR. nothing in this
/// crate can check that: the tables live in crates that depend on this one.
/// `netform_cli`'s `detect_guard_coverage` suite sees all four and asserts it.
const OTHER_DIALECT_INTERFACE_PREFIXES: &[&str] = &[
    "appgigabitethernet",
    "bdi",
    "ethernet",
    "fabric",
    "fastethernet",
    "fivegigabitethernet",
    "fortygigabitethernet",
    "fourhundredgig",
    "gigabitethernet",
    "hundredgigabitethernet",
    "hundredgige",
    "loopback",
    "management",
    "mgmt",
    "nve",
    "port-channel",
    "serial",
    "tengigabitethernet",
    "tunnel",
    "twentyfivegige",
    "twogigabitethernet",
    "twohundredgig",
    "vlan",
    "vxlan",
];

/// returns `true` if `lower` — an already-lowercased interface name — starts
/// with an interface type another supported dialect spells.
fn is_other_dialect_interface_name(lower: &str) -> bool {
    OTHER_DIALECT_INTERFACE_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

/// returns `true` if `name` ends in four `/`-separated numeric components,
/// ignoring any `.subinterface` suffix.
fn has_four_part_slot(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or(name);
    let slots: Vec<&str> = base.split('/').collect();

    slots.len() == 4
        && slots[1..].iter().all(|slot| slot.parse::<u32>().is_ok())
        && slots[0]
            .trim_start_matches(|c: char| !c.is_ascii_digit())
            .parse::<u32>()
            .is_ok()
}

/// score the Huawei VRP signals `line` carries, excluding the interface names
/// [`is_vrp_interface_name`] scores from the shared `interface` chain.
fn vrp_signal_score(line: &str) -> i32 {
    let mut score = 0;

    if line.starts_with("undo ") {
        score += WEAK_SIGNAL;
    }
    if line.starts_with("sysname ") {
        score += STRONG_SIGNAL;
    }
    if line.starts_with("vlan batch ") {
        score += STRONG_SIGNAL;
    }
    if line.starts_with("ip vpn-instance ") {
        score += STRONG_SIGNAL;
    }
    if line.starts_with("user-interface ") {
        score += STRONG_SIGNAL;
    }
    if line.starts_with("ipv4-family") || line.starts_with("ipv6-family") {
        score += MODERATE_SIGNAL;
    }
    if line.starts_with("port link-type ") || line.starts_with("port default vlan ") {
        score += MODERATE_SIGNAL;
    }
    // VRP's `neighbor <ip> remote-as <asn>`.
    if line.starts_with("peer ") && line.contains(" as-number ") {
        score += MODERATE_SIGNAL;
    }

    score
}

/// returns `true` if `name` is an interface type only Huawei VRP spells.
///
/// the shared families VRP also parses (`GigabitEthernet`, `LoopBack`,
/// `Tunnel`, `Pos`, `Null`, `Virtual-Template`) are deliberately absent.
fn is_vrp_interface_name(name: &str) -> bool {
    const VRP_ONLY_PREFIXES: [&str; 8] = [
        "vlanif",
        "eth-trunk",
        "ip-trunk",
        "xgigabitethernet",
        "meth",
        "25ge",
        "40ge",
        "100ge",
    ];

    let lower = name.to_ascii_lowercase();

    VRP_ONLY_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
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
    fn detect_vrp() {
        assert_eq!(
            detect_dialect(VRP_FIXTURE),
            DialectHint::Named("vrp".into())
        );
    }

    #[test]
    fn detect_vrp_wins_over_the_iosxe_names_it_shares() {
        let input = "\
sysname CE-ACCESS-01
vlan batch 10 20
interface GigabitEthernet0/0/1
 port link-type access
 port default vlan 10
 ip address 10.0.10.1 255.255.255.0
";
        assert_eq!(detect_dialect(input), DialectHint::Named("vrp".into()));
    }

    #[test]
    fn detect_vrp_from_its_own_interface_families() {
        for iface in [
            "Vlanif10",
            "Eth-Trunk1",
            "Ip-Trunk1",
            "XGigabitEthernet0/0/1",
            "MEth0/0/1",
            "25GE1/0/1",
            "40GE1/0/1",
            "100GE1/0/1",
        ] {
            let input = format!("interface {iface}\n description edge\n");
            assert_eq!(
                detect_dialect(&input),
                DialectHint::Named("vrp".into()),
                "`interface {iface}` should read as VRP",
            );
        }
    }

    #[test]
    fn detect_vrp_reads_its_own_families_in_either_case() {
        for iface in ["vlanif10", "eth-trunk1", "100ge1/0/1"] {
            let input = format!("interface {iface}\n description edge\n");
            assert_eq!(
                detect_dialect(&input),
                DialectHint::Named("vrp".into()),
                "`interface {iface}` should read as VRP in lowercase too",
            );
        }
    }

    #[test]
    fn detect_vrp_ignores_the_hash_separators() {
        let input = "#\n#\n#\n#\n#\n";
        assert_eq!(detect_dialect(input), DialectHint::Generic);
    }

    #[test]
    fn detect_generic_when_eos_and_vrp_interfaces_mix() {
        let input = "\
ip access-list ACL-EDGE-IN
   10 permit tcp 10.10.1.0/24 any eq https
interface Management1
interface Vlanif10
 description edge
";
        assert_eq!(detect_dialect(input), DialectHint::Generic);
    }

    #[test]
    fn vrp_scores_the_constructs_the_ios_family_spells_otherwise() {
        for line in [
            "undo portswitch",
            "sysname CE-ACCESS-01",
            "vlan batch 10 20",
            "ip vpn-instance BLUE",
            "user-interface vty 0 4",
            "ipv4-family unicast",
            "ipv6-family vpn-instance BLUE",
            "port link-type access",
            "port default vlan 10",
            "peer 10.0.0.2 as-number 65001",
        ] {
            assert!(vrp_signal_score(line) > 0, "`{line}` scores no VRP signal");
        }
    }

    #[test]
    fn the_ios_family_spellings_score_no_vrp_signal() {
        for line in [
            "no switchport",
            "hostname c9500-lab",
            "vlan 10",
            "vrf definition BLUE",
            "line vty 0 4",
            "address-family ipv4 unicast",
            "switchport mode access",
            "switchport access vlan 10",
            "neighbor 10.0.0.2 remote-as 65001",
        ] {
            assert_eq!(vrp_signal_score(line), 0, "`{line}` scores a VRP signal");
        }
    }

    #[test]
    fn vrp_signals_leave_the_other_dialects_where_they_were() {
        for (name, input) in [
            ("iosxe", IOSXE_FIXTURE),
            ("nxos", NXOS_FIXTURE),
            ("eos", EOS_FIXTURE),
            ("junos", JUNOS_FIXTURE),
            ("fortios", FORTIOS_FIXTURE),
            ("iosxr", IOSXR_FIXTURE),
            ("iosxe", C9500_32C_FIXTURE),
        ] {
            assert_eq!(
                detect_dialect(input),
                DialectHint::Named(name.into()),
                "{name} fixture no longer detects as itself",
            );

            let lines: Vec<&str> = input.lines().map(str::trim).collect();
            for (&line, scorable) in lines.iter().zip(scorable_lines(&lines)) {
                if !scorable {
                    continue;
                }
                assert_eq!(
                    vrp_signal_score(line),
                    0,
                    "`{line}` in the {name} fixture scores VRP",
                );
                if let Some(iface) = line.strip_prefix("interface ") {
                    assert!(
                        !is_vrp_interface_name(iface),
                        "`{line}` in the {name} fixture reads as a VRP interface",
                    );
                }
            }
        }
    }

    const IOSXR_FIXTURE: &str = "\
interface Bundle-Ether10
 ipv4 address 192.0.2.1 255.255.255.252
interface TenGigE0/0/0/0
 bundle id 10 mode active
route-policy PASS-ALL
  pass
end-policy
prefix-set CUSTOMER-PFX
  10.0.0.0/8 le 24
end-set
";

    const VRP_FIXTURE: &str = "\
#
sysname CE-ACCESS-01
#
vlan batch 10 20
#
ip vpn-instance BLUE
 ipv4-family
  route-distinguisher 65000:100
#
interface Vlanif10
 ip address 10.0.10.1 255.255.255.0
#
interface Eth-Trunk1
 port link-type trunk
#
interface GigabitEthernet0/0/2
 undo portswitch
 ip binding vpn-instance BLUE
#
bgp 65000
 peer 10.0.0.2 as-number 65001
#
user-interface vty 0 4
 authentication-mode aaa
#
return
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
    fn scorable_lines_does_not_reopen_a_banner_inside_a_banner_body() {
        let lines = [
            "banner motd ^C",
            "banner set by netops on 2026-01-01",
            "^C",
            "interface GigabitEthernet1/0/1",
            " standby 1 ip 192.0.2.254",
        ];
        assert_eq!(scorable_lines(&lines), [true, false, false, true, true]);
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
}
