//! Score-based dialect auto-detection from configuration text.
//!
//! Scans lines for dialect-specific patterns, accumulates per-dialect scores,
//! and returns the highest-scoring dialect as a [`DialectHint`].  Falls back to
//! [`DialectHint::Generic`] when the top score is too low or when the margin
//! between the top two candidates is too narrow.
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

use crate::{DialectHint, Document, parse_generic};

/// Detect the likely network-device dialect from configuration text.
///
/// Returns a [`DialectHint`] identifying the detected dialect:
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

        // --- FortiOS signals ---
        // `config <section>` / `end` block structure is unique to FortiOS.
        if line.starts_with("config ")
            && !line.contains('{')
            && line.split_whitespace().count() >= 2
        {
            fortios += 3;
        }
        if line.starts_with("edit ") {
            fortios += 3;
        }
        if line == "end" {
            fortios += 2;
        }
        if line == "next" {
            fortios += 2;
        }
        if line.starts_with("set ") || line.starts_with("unset ") {
            let second = line.split_whitespace().nth(1).unwrap_or("");
            if is_junos_stanza_name(second) {
                // `set interfaces ...`, `set protocols ...` etc — Junos set-style.
                junos += 3;
            } else {
                // Plain `set <field> <value>` leans FortiOS (inside config blocks).
                fortios += 1;
            }
        }

        // --- Junos signals ---
        // Brace-and-semicolon syntax is highly distinctive.
        if line.ends_with('{') {
            junos += 2;
        }
        if line == "}" || line.ends_with("};") {
            junos += 2;
        }
        if line.ends_with(';') && !line.ends_with("};") {
            junos += 1;
        }
        // Junos-specific stanza names at top-level (hierarchical style).
        if is_junos_stanza_name(line.split_whitespace().next().unwrap_or("")) {
            junos += 3;
        }

        // --- NX-OS signals ---
        // `feature <name>` is unique to NX-OS among supported dialects.
        if line.starts_with("feature ") {
            nxos += 3;
        }
        // Slot/port interfaces: Ethernet1/1, port-channel1, etc.
        if line.starts_with("interface ") {
            let iface = line.trim_start_matches("interface ");
            if iface.starts_with("Ethernet") && iface.contains('/') {
                // NX-OS uses plain Ethernet with slot/port (Ethernet1/1).
                // IOS XE uses GigabitEthernet, TenGigabitEthernet etc. with slashes.
                nxos += 3;
            } else if iface.starts_with("Ethernet") || iface.starts_with("Management") {
                // No slot → could be EOS.
                eos += 2;
            }
        }
        if line.starts_with("vpc ") {
            nxos += 3;
        }
        if line.starts_with("role name ") {
            nxos += 2;
        }

        // --- IOS XE signals ---
        // `ip access-list extended` is a strong IOS XE marker.
        if line.starts_with("ip access-list extended ") {
            iosxe += 3;
        }
        // Dotted subnet masks with `ip address` (IOS XE uses masks, not CIDR).
        if line.starts_with("ip address ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 && looks_like_dotted_mask(parts[3]) {
                iosxe += 2;
            }
        }
        // `network ... mask ...` in BGP address-family.
        if line.contains(" mask ") && line.starts_with("network ") {
            iosxe += 2;
        }
        // Wildcard masks in ACL permit/deny lines.
        if (line.starts_with("permit ") || line.starts_with("deny "))
            && line.split_whitespace().any(looks_like_dotted_mask)
        {
            iosxe += 1;
        }

        // --- EOS signals ---
        if line.starts_with("ip access-list ") && !line.contains("extended") {
            eos += 2;
        }
        // Numbered ACL entries (EOS style: `10 permit ...`).
        if let Some(first) = line.split_whitespace().next()
            && first.parse::<u32>().is_ok()
            && (line.contains(" permit ") || line.contains(" deny "))
        {
            eos += 1;
        }
        // EOS uses CIDR notation for ip addresses (no dotted mask).
        if line.starts_with("ip address ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[2].contains('/') {
                eos += 2;
            }
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

    // Require a minimum score and a clear margin (2×) over the runner-up.
    if best_score < 3 {
        return DialectHint::Generic;
    }
    if best_score < second_score * 2 {
        return DialectHint::Generic;
    }

    DialectHint::Named(best_name.to_string())
}

/// Parse input with automatic dialect detection.
///
/// Runs [`detect_dialect`] to identify the dialect from the input text, then
/// parses with the generic parser and sets the detected [`DialectHint`] in the
/// document metadata.
///
/// For full dialect-specific parsing (Junos brace handling, FortiOS block
/// structure, etc.), call [`detect_dialect`] directly and dispatch to the
/// appropriate dialect parser.
pub fn auto_parse(input: &str) -> Document {
    let hint = detect_dialect(input);
    let mut doc = parse_generic(input);
    doc.metadata.dialect_hint = hint;
    doc
}

/// Returns `true` if `name` is a well-known Junos top-level stanza name.
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

/// Returns `true` if `s` looks like a dotted-decimal subnet or wildcard mask
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
        // Minimal content with weak signals from multiple dialects.
        let input = "\
set hostname myrouter
interface Ethernet1
";
        // Both junos/fortios and eos get mild scores — should fall back.
        assert_eq!(detect_dialect(input), DialectHint::Generic);
    }

    #[test]
    fn detect_generic_on_exact_tie() {
        // Craft input where NX-OS and EOS each score exactly 3.
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
    fn auto_parse_sets_dialect_hint() {
        let input = "\
interfaces {
    ge-0/0/0 {
        mtu 9216;
    }
}
";
        let doc = auto_parse(input);
        assert_eq!(
            doc.metadata.dialect_hint,
            DialectHint::Named("junos".into())
        );
    }

    #[test]
    fn auto_parse_generic_fallback() {
        let doc = auto_parse("hostname router\n");
        assert_eq!(doc.metadata.dialect_hint, DialectHint::Generic);
    }

    #[test]
    fn auto_parse_preserves_content() {
        let input = "interface Ethernet1\n  description uplink\n";
        let doc = auto_parse(input);
        assert_eq!(doc.render(), input);
    }
}
