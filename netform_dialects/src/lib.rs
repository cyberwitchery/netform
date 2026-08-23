//! the registry of network configuration dialects netform supports.
//!
//! a vendor is an entry in [`REGISTRY`], not a crate.  most vendors are
//! described entirely as data — an interface-type table, a VRF keyword, a set
//! of router protocols and a list of [`rules::KeyRule`]s — and the IOS-like
//! parser is driven from that.  vendors whose grammar needs real code (Junos'
//! brace blocks, FortiOS' `config`/`edit` terminators) keep their own parser
//! and appear here through [`DialectEntry::parse`], so the registry stays the
//! single list of vendors either way.
//!
//! # Example
//!
//! ```rust
//! let eos = netform_dialects::find("eos").expect("eos is registered");
//! let doc = (eos.parse)("interface Ethernet1\n   description Uplink\n");
//! assert_eq!(doc.render(), "interface Ethernet1\n   description Uplink\n");
//! ```

pub mod rules;

pub mod eos;
pub mod iosxe;
pub mod iosxr;
pub mod nxos;

use netform_ir::{Document, IosKeyHintConfig, ParsedLineParts, ios_family_key_hint};
use rules::{KeyRule, rule_key_hint};

/// the data an IOS-family vendor needs to derive key hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IosRules {
    /// interface type prefixes in canonical lowercase form,
    /// longest-prefix-first (see `netform_ir::parse_interface`).
    pub interface_types: &'static [&'static str],
    /// the VRF sub-command keyword (`"instance"`, `"definition"`, `"context"`).
    pub vrf_keyword: &'static str,
    /// router protocols beyond BGP and OSPF whose second argument belongs in
    /// the hint.
    pub extra_router_protos: &'static [&'static str],
    /// vendor-specific rules, tried after the shared IOS-family arms.
    pub key_rules: &'static [KeyRule],
}

impl IosRules {
    /// this vendor's configuration for [`ios_family_key_hint`].
    pub const fn key_hint_config(&self) -> IosKeyHintConfig {
        IosKeyHintConfig {
            interface_types: self.interface_types,
            vrf_keyword: self.vrf_keyword,
            extra_router_protos: self.extra_router_protos,
        }
    }

    /// derive a stable identity key for one of this vendor's configuration
    /// lines.
    pub fn key_hint(&self, parsed: Option<&ParsedLineParts>) -> Option<String> {
        if let Some(hint) = ios_family_key_hint(parsed, &self.key_hint_config()) {
            return Some(hint);
        }
        rule_key_hint(self.key_rules, parsed)
    }
}

/// one vendor netform can parse.
#[derive(Debug, Clone, Copy)]
pub struct DialectEntry {
    /// the vendor's name, as `--dialect` spells it and as
    /// `netform_ir::DialectHint::Named` carries it.
    pub name: &'static str,
    /// parse configuration text as this vendor.
    pub parse: fn(&str) -> Document,
    /// the vendor's key-hint data, or `None` for a vendor whose parser is code.
    pub rules: Option<&'static IosRules>,
    /// a short configuration excerpt this vendor's detection signals should
    /// claim, and no other vendor's should.
    pub sample: &'static str,
}

impl PartialEq for DialectEntry {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for DialectEntry {}

/// every vendor netform supports.
pub const REGISTRY: &[DialectEntry] = &[
    DialectEntry {
        name: "eos",
        parse: eos::parse,
        rules: Some(&eos::RULES),
        sample: eos::SAMPLE,
    },
    DialectEntry {
        name: "fortios",
        parse: netform_dialect_fortios::parse_fortios,
        rules: None,
        sample: FORTIOS_SAMPLE,
    },
    DialectEntry {
        name: "iosxe",
        parse: iosxe::parse,
        rules: Some(&iosxe::RULES),
        sample: iosxe::SAMPLE,
    },
    DialectEntry {
        name: "iosxr",
        parse: iosxr::parse,
        rules: Some(&iosxr::RULES),
        sample: iosxr::SAMPLE,
    },
    DialectEntry {
        name: "junos",
        parse: netform_dialect_junos::parse_junos,
        rules: None,
        sample: JUNOS_SAMPLE,
    },
    DialectEntry {
        name: "nxos",
        parse: nxos::parse,
        rules: Some(&nxos::RULES),
        sample: nxos::SAMPLE,
    },
];

const FORTIOS_SAMPLE: &str = "\
config system global
    set hostname \"fw-edge-01\"
    set timezone 26
end
config firewall address
    edit \"LAN\"
        set subnet 10.0.0.0 255.255.255.0
    next
end
";

const JUNOS_SAMPLE: &str = "\
interfaces {
    ge-0/0/0 {
        description uplink-core-a;
        unit 0 {
            family inet {
                address 192.0.2.2/30;
            }
        }
    }
}
protocols {
    bgp {
        group underlay {
            peer-as 65001;
        }
    }
}
";

/// look up a vendor by the name `--dialect` spells.
pub fn find(name: &str) -> Option<&'static DialectEntry> {
    REGISTRY.iter().find(|entry| entry.name == name)
}

/// every registered vendor's name.
pub fn names() -> impl Iterator<Item = &'static str> {
    REGISTRY.iter().map(|entry| entry.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::DialectHint;
    use netform_ir::detect::detect_dialect;
    use netform_ir::parse_ios_like_parts;
    use rules::KeyRuleAction;

    #[test]
    fn registry_names_are_unique_and_sorted() {
        let names: Vec<&str> = names().collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names, sorted);
    }

    #[test]
    fn every_entry_is_reachable_by_name() {
        for entry in REGISTRY {
            assert_eq!(find(entry.name), Some(entry));
        }
    }

    #[test]
    fn every_entry_parses_its_own_sample_losslessly() {
        for entry in REGISTRY {
            let doc = (entry.parse)(entry.sample);
            assert_eq!(doc.render(), entry.sample, "{} sample", entry.name);
            assert_eq!(
                doc.metadata.dialect_hint,
                DialectHint::Named(entry.name.into()),
                "{} sample",
                entry.name,
            );
        }
    }

    #[test]
    fn every_sample_detects_as_its_own_vendor() {
        for entry in REGISTRY {
            assert_eq!(
                detect_dialect(entry.sample),
                DialectHint::Named(entry.name.into()),
                "{} sample no longer detects as itself",
                entry.name,
            );
        }
    }

    /// the generic form of the per-vendor proof: no vendor's detection signals
    /// fire on any line of any other vendor's configuration.
    #[test]
    fn no_vendors_signals_reach_another_vendors_lines() {
        for entry in REGISTRY {
            let claimed = DialectHint::Named(entry.name.into());
            for other in REGISTRY {
                if other.name == entry.name {
                    continue;
                }
                for line in other.sample.lines() {
                    let input = format!("{line}\n");
                    assert_ne!(
                        detect_dialect(&input),
                        claimed,
                        "`{line}` from the {} sample scores as {}",
                        other.name,
                        entry.name,
                    );
                }
            }
        }
    }

    #[test]
    fn shared_arms_win_over_a_vendor_rule_on_the_same_head() {
        const RULES: IosRules = IosRules {
            interface_types: &["ethernet"],
            vrf_keyword: "instance",
            extra_router_protos: &[],
            key_rules: &[KeyRule {
                head: "interface",
                guards: &[],
                action: KeyRuleAction::key("iface", &[0]),
            }],
        };

        let parsed = parse_ios_like_parts("interface Ethernet1");
        assert_eq!(
            rule_key_hint(RULES.key_rules, parsed.as_ref()),
            Some("iface:Ethernet1".into()),
        );
        assert_eq!(
            RULES.key_hint(parsed.as_ref()),
            Some("interface:ethernet:1".into()),
        );
    }
}
