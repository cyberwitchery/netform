//! the registry of network configuration dialects netform supports.
//!
//! a vendor is an entry in [`REGISTRY`], not a crate.  most vendors are
//! described entirely as data — an interface-type table, a VRF keyword, a set
//! of router protocols, a list of [`rules::KeyRule`]s and the
//! [`detect::Signal`]s that identify its configuration text — and
//! the IOS-like parser and [`detect_dialect`] are driven from that.  vendors
//! whose grammar needs real code (Junos' brace blocks, FortiOS'
//! `config`/`edit` terminators) keep their own parser and appear here through
//! [`DialectEntry::parse`], so the registry stays the single list of vendors
//! either way.
//!
//! # Example
//!
//! ```rust
//! use netform_ir::DialectHint;
//!
//! let eos = netform_dialects::find("eos").expect("eos is registered");
//! let doc = (eos.parse)("interface Ethernet1\n   description Uplink\n");
//! assert_eq!(doc.render(), "interface Ethernet1\n   description Uplink\n");
//!
//! assert_eq!(
//!     netform_dialects::detect_dialect(eos.sample),
//!     DialectHint::Named("eos".into()),
//! );
//! ```

pub mod detect;
pub mod ios_like;
pub mod rules;

pub mod eos;
pub mod fortios;
pub mod iosxe;
pub mod iosxr;
pub mod junos;
pub mod nxos;

use detect::{Signal, SignalTable};
use ios_like::SHARED_RULES;
use netform_ir::{DialectHint, Document, ParsedLineParts, parse_generic};
use rules::KeyRule;

/// the data an IOS-family vendor needs to derive key hints.
#[derive(Debug, Clone, Copy)]
pub struct IosRules {
    /// interface type prefixes in canonical lowercase form,
    /// longest-prefix-first (see `ios_like::parse_interface`).
    pub interface_types: &'static [&'static str],
    /// the VRF sub-command keyword (`"instance"`, `"definition"`, `"context"`).
    pub vrf_keyword: &'static str,
    /// router protocols beyond BGP and OSPF whose second argument belongs in
    /// the hint.
    pub extra_router_protos: &'static [&'static str],
    /// whether a static route's distance is part of its identity.
    pub route_distance_is_identity: bool,
    /// vendor-specific rules, tried before [`ios_like::SHARED_RULES`], so a
    /// vendor rule that matches a line overrides the shared rule for its head.
    pub key_rules: &'static [KeyRule],
}

impl IosRules {
    /// derive a stable identity key for one of this vendor's configuration
    /// lines: the first rule in [`IosRules::key_rules`], then
    /// [`ios_like::SHARED_RULES`], that matches the line decides it.
    pub fn key_hint(&self, parsed: Option<&ParsedLineParts>) -> Option<String> {
        let parsed = parsed?;
        let (head, args) = (parsed.head.as_str(), parsed.args.as_slice());
        self.key_rules
            .iter()
            .chain(SHARED_RULES)
            .find(|rule| rule.applies(head, args))
            .and_then(|rule| rule.render(args, self))
    }
}

/// one vendor netform can parse, or [`GENERIC`].
#[derive(Debug, Clone, Copy)]
pub struct DialectEntry {
    /// the vendor's name, as `--dialect` spells it and as
    /// `netform_ir::DialectHint::Named` carries it.
    pub name: &'static str,
    /// parse configuration text as this vendor.
    pub parse: fn(&str) -> Document,
    /// the vendor's key-hint data, or `None` for a vendor whose parser is code.
    pub rules: Option<&'static IosRules>,
    /// the patterns [`detect_dialect`] scores this vendor on.
    pub signals: &'static [Signal],
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
        signals: eos::SIGNALS,
        sample: eos::SAMPLE,
    },
    DialectEntry {
        name: "fortios",
        parse: fortios::parse,
        rules: None,
        signals: fortios::SIGNALS,
        sample: fortios::SAMPLE,
    },
    DialectEntry {
        name: "iosxe",
        parse: iosxe::parse,
        rules: Some(&iosxe::RULES),
        signals: iosxe::SIGNALS,
        sample: iosxe::SAMPLE,
    },
    DialectEntry {
        name: "iosxr",
        parse: iosxr::parse,
        rules: Some(&iosxr::RULES),
        signals: iosxr::SIGNALS,
        sample: iosxr::SAMPLE,
    },
    DialectEntry {
        name: "junos",
        parse: junos::parse,
        rules: None,
        signals: junos::SIGNALS,
        sample: junos::SAMPLE,
    },
    DialectEntry {
        name: "nxos",
        parse: nxos::parse,
        rules: Some(&nxos::RULES),
        signals: nxos::SIGNALS,
        sample: nxos::SAMPLE,
    },
];

/// netform_ir's vendor-neutral parser, the entry [`find`] returns for
/// `generic`.  it is not in [`REGISTRY`], so [`detect_dialect`] never picks it.
pub const GENERIC: DialectEntry = DialectEntry {
    name: "generic",
    parse: parse_generic,
    rules: None,
    signals: &[],
    sample: "",
};

/// every entry [`find`] resolves: [`GENERIC`], then each [`REGISTRY`] vendor.
pub fn entries() -> impl Iterator<Item = &'static DialectEntry> {
    std::iter::once(&GENERIC).chain(REGISTRY)
}

/// look up a parser by the name `--dialect` spells: `generic` or a
/// registered vendor.
pub fn find(name: &str) -> Option<&'static DialectEntry> {
    entries().find(|entry| entry.name == name)
}

/// every registered vendor's name.
pub fn names() -> impl Iterator<Item = &'static str> {
    REGISTRY.iter().map(|entry| entry.name)
}

/// detect which registered vendor wrote `input`.
///
/// scores every entry in [`REGISTRY`] on its own signals and returns the
/// winner, or [`DialectHint::Generic`] when none of them is a confident enough
/// match.  see [`detect`] for the scoring rules.
pub fn detect_dialect(input: &str) -> DialectHint {
    let tables: Vec<SignalTable> = REGISTRY
        .iter()
        .map(|entry| SignalTable {
            name: entry.name,
            signals: entry.signals,
        })
        .collect();

    detect::detect_dialect(input, &tables)
}

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::parse_parts_with_punctuation;
    use rules::{ArgGuard, KeyRuleAction};

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
        for entry in entries() {
            assert_eq!(find(entry.name), Some(entry));
        }
    }

    #[test]
    fn generic_is_found_by_name_but_is_not_a_registered_vendor() {
        assert!(!REGISTRY.contains(&GENERIC));
        let generic = find("generic").expect("generic resolves");
        for entry in REGISTRY {
            let doc = (generic.parse)(entry.sample);
            assert_eq!(
                doc.metadata.dialect_hint,
                DialectHint::Generic,
                "{} sample",
                entry.name,
            );
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
    fn a_vendor_rule_overrides_the_shared_rule_on_its_head() {
        const RULES: IosRules = IosRules {
            interface_types: &["ethernet"],
            vrf_keyword: "instance",
            extra_router_protos: &[],
            route_distance_is_identity: false,
            key_rules: &[KeyRule {
                head: "interface",
                guards: &[ArgGuard::new(0, &["Ethernet1"])],
                action: KeyRuleAction::key("iface", &[0]),
            }],
        };

        for (line, expected) in [
            ("interface Ethernet1", "iface:Ethernet1"),
            ("interface Ethernet2", "interface:ethernet:2"),
        ] {
            let parsed = parse_parts_with_punctuation(line, &[]);
            assert_eq!(RULES.key_hint(parsed.as_ref()).as_deref(), Some(expected));
        }
    }

    /// the IOS-family vendors: every entry whose key hints come from [`IosRules`].
    fn ios_family() -> impl Iterator<Item = (&'static str, &'static IosRules)> {
        REGISTRY
            .iter()
            .filter_map(|entry| Some((entry.name, entry.rules?)))
    }

    #[test]
    fn ios_family_shared_key_hints() {
        for (line, expected) in [
            ("interface", None),
            ("vlan 100", Some("vlan:100")),
            ("vlan 1-100,200,300-400", Some("vlan:1-100,200,300-400")),
            ("vrf MGMT", Some("vrf:MGMT")),
            ("router bgp 65000", Some("router:bgp:65000")),
            ("router ospf 1", Some("router:ospf:1")),
            ("router ospf", Some("router:ospf")),
            ("router isis", Some("router:isis")),
            (
                "route-map REDISTRIBUTE permit 10",
                Some("route-map:REDISTRIBUTE:permit:10"),
            ),
            ("route-map EXPORT deny", Some("route-map:EXPORT:deny")),
            ("ip access-list ACL-MGMT", Some("ip-access-list:ACL-MGMT")),
            (
                "ip access-list extended BLOCK-RFC1918",
                Some("ip-access-list:extended:BLOCK-RFC1918"),
            ),
            (
                "ip access-list standard ALLOW-SNMP",
                Some("ip-access-list:standard:ALLOW-SNMP"),
            ),
            (
                "ip prefix-list DEFAULT-ONLY",
                Some("prefix-list:DEFAULT-ONLY"),
            ),
            (
                "ip prefix-list PL seq 5 permit 10.0.0.0/8",
                Some("prefix-list:PL:5"),
            ),
            ("ip prefix-list PL permit 10.0.0.0/8", None),
            (
                "ip prefix-list PL description uplinks",
                Some("prefix-list:PL:description"),
            ),
            ("ip community-list standard CL permit 65000:1", None),
            // static routes key on vrf, destination and next hop
            (
                "ip route 10.0.0.0/8 192.168.1.1",
                Some("ip-route:10.0.0.0/8:192.168.1.1"),
            ),
            (
                "ip route 10.0.0.0 255.0.0.0 192.168.1.1 tag 5",
                Some("ip-route:10.0.0.0/8:192.168.1.1"),
            ),
            (
                "ip route vrf MGMT 0.0.0.0/0 10.0.0.1 permanent",
                Some("ip-route:MGMT:0.0.0.0/0:10.0.0.1"),
            ),
            (
                "ip route vrf MGMT 0.0.0.0 0.0.0.0 10.0.0.1 metric 10",
                Some("ip-route:MGMT:0.0.0.0/0:10.0.0.1"),
            ),
            (
                "ip route 10.0.0.0 255.0.0.0 GigabitEthernet0/0 192.168.1.1 name X 200",
                Some("ip-route:10.0.0.0/8:GigabitEthernet0/0:192.168.1.1"),
            ),
            (
                "ip route 0.0.0.0 0.0.0.0 192.168.1.1 track 1",
                Some("ip-route:0.0.0.0/0:192.168.1.1"),
            ),
            ("ip route static bfd GigabitEthernet0/0 192.168.1.1", None),
            ("monitor session 1", Some("monitor-session:1")),
            ("monitor copp-system-p-policy", None),
            ("monitor capture CAP1", None),
            ("ntp server 10.0.0.1", Some("ntp:server:10.0.0.1")),
            ("ntp peer 10.0.0.2", Some("ntp:peer:10.0.0.2")),
            ("ntp source-interface Management1", None),
            ("ntp source GigabitEthernet0/0/0", None),
            ("class-map match-all VOICE", Some("class-map:VOICE")),
            ("policy-map QOS-POLICY", Some("policy-map:QOS-POLICY")),
            // typed class-maps and policy-maps key on type and name (0.10.1)
            (
                "class-map type qos match-any CM-VOICE",
                Some("class-map:qos:CM-VOICE"),
            ),
            (
                "class-map type pbr match-any CM-PBR",
                Some("class-map:pbr:CM-PBR"),
            ),
            (
                "class-map type inspect match-any CM-INSPECT",
                Some("class-map:inspect:CM-INSPECT"),
            ),
            (
                "class-map type control subscriber match-all CM-SUB",
                Some("class-map:control:subscriber:CM-SUB"),
            ),
            (
                "class-map type control-plane match-any copp-s-bgp",
                Some("class-map:control-plane:copp-s-bgp"),
            ),
            (
                "class-map type queuing match-any c-out-q3",
                Some("class-map:queuing:c-out-q3"),
            ),
            ("policy-map type pbr PM-PBR", Some("policy-map:pbr:PM-PBR")),
            (
                "policy-map type quality-of-service PM-QOS",
                Some("policy-map:quality-of-service:PM-QOS"),
            ),
            (
                "policy-map type inspect PM-INSPECT",
                Some("policy-map:inspect:PM-INSPECT"),
            ),
            (
                "policy-map type control subscriber PM-SUB",
                Some("policy-map:control:subscriber:PM-SUB"),
            ),
            (
                "policy-map type control-plane copp-system-p",
                Some("policy-map:control-plane:copp-system-p"),
            ),
            (
                "policy-map type network-qos default-nq-policy",
                Some("policy-map:network-qos:default-nq-policy"),
            ),
            (
                "spanning-tree vlan 1-100 priority 4096",
                Some("spanning-tree:vlan:1-100:priority"),
            ),
            // iosxe claims `crypto` and falls through to the shared arms
            (
                "crypto ikev2 proposal PROP-1",
                Some("crypto:ikev2:proposal:PROP-1"),
            ),
            (
                "crypto ipsec transform-set MY-TSET",
                Some("crypto:ipsec:transform-set:MY-TSET"),
            ),
            ("crypto map MY-MAP 10", Some("crypto:map:MY-MAP:10")),
            ("line vty 0 4", Some("line:vty:0:4")),
            ("line con 0", Some("line:con:0")),
            ("hostname leaf-01", None),
        ] {
            for (name, rules) in ios_family() {
                assert_eq!(
                    rules
                        .key_hint(parse_parts_with_punctuation(line, &[]).as_ref())
                        .as_deref(),
                    expected,
                    "{name}: {line}",
                );
            }
        }
    }

    #[test]
    fn ios_family_router_hints_keep_the_instance_only_for_extra_router_protos() {
        for (name, rules) in ios_family() {
            for (line, proto, with_instance) in [
                ("router eigrp 100", "eigrp", "router:eigrp:100"),
                (
                    "router eigrp ENTERPRISE",
                    "eigrp",
                    "router:eigrp:ENTERPRISE",
                ),
                ("router isis AREA-A", "isis", "router:isis:AREA-A"),
            ] {
                let expected = if rules.extra_router_protos.contains(&proto) {
                    with_instance.to_string()
                } else {
                    format!("router:{proto}")
                };
                assert_eq!(
                    rules.key_hint(parse_parts_with_punctuation(line, &[]).as_ref()),
                    Some(expected),
                    "{name}: {line}",
                );
            }
        }
    }

    #[test]
    fn ios_family_key_hint_is_none_on_empty() {
        for (name, rules) in ios_family() {
            assert_eq!(rules.key_hint(None), None, "{name}");
        }
    }
}
