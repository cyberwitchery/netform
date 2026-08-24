//! score-based dialect auto-detection from configuration text.
//!
//! this module is the scoring engine: the weights, the thresholds, the
//! line-shape tests a vendor's signals are written in, and the winner/margin
//! arithmetic.  the signals are data the caller supplies — one [`SignalTable`]
//! per vendor — and live with the rest of that vendor's description;
//! `netform_dialects` holds netform's and wraps this in a zero-argument
//! `detect_dialect` over its registry.
//!
//! [`detect_dialect`] accumulates a per-vendor score over the scorable lines
//! and returns the highest-scoring vendor as a [`DialectHint`].  the winner
//! must both meet `MIN_CONFIDENCE_SCORE` and outscore the runner-up by
//! `MARGIN_FACTOR`; otherwise the result is [`DialectHint::Generic`].
//!
//! # Example
//!
//! ```rust
//! use netform_ir::DialectHint;
//! use netform_ir::detect::{MODERATE_SIGNAL, Signal, SignalTable, Test, detect_dialect};
//!
//! const BRACES: &[Signal] = &[Signal {
//!     weight: MODERATE_SIGNAL,
//!     tests: &[Test::EndsWithAny(&["{"])],
//! }];
//!
//! let tables = [SignalTable { name: "junos", signals: BRACES }];
//! let junos_cfg = "interfaces {\n    ge-0/0/0 {\n        unit 0 {\n";
//! assert_eq!(
//!     detect_dialect(junos_cfg, &tables),
//!     DialectHint::Named("junos".into()),
//! );
//!
//! assert_eq!(detect_dialect("", &tables), DialectHint::Generic);
//! ```

use crate::{DialectHint, ios_like_literal_region};

/// score for a highly distinctive, dialect-unique pattern (e.g. FortiOS
/// `config <section>`, NX-OS `feature <name>`, Junos top-level stanza names).
pub const STRONG_SIGNAL: i32 = 3;

/// score for a moderately distinctive pattern (e.g. FortiOS `end`/`next`,
/// Junos brace open/close, EOS non-slot and IOS XE speed-prefixed Ethernet
/// interfaces).
pub const MODERATE_SIGNAL: i32 = 2;

/// score for a pattern that weakly suggests a dialect (e.g. Junos trailing
/// semicolons, FortiOS plain `set <field>`, IOS XE wildcard masks in ACLs).
pub const WEAK_SIGNAL: i32 = 1;

/// minimum total score a dialect must reach to be considered detected (at
/// least one strong signal or multiple weaker ones).  below this threshold,
/// the input is too short or too ambiguous to identify.
const MIN_CONFIDENCE_SCORE: i32 = 3;

/// the winning dialect must outscore the runner-up by at least this factor.
/// a value of 2 means the winner needs ≥ 2× the runner-up's score.
const MARGIN_FACTOR: i32 = 2;

/// one thing a scorable line can be asked about.
///
/// a [`Signal`] holds several; the line must satisfy all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Test {
    /// the line starts with one of these.
    StartsWithAny(&'static [&'static str]),
    /// the line is exactly one of these.
    IsAny(&'static [&'static str]),
    /// the line ends with one of these.
    EndsWithAny(&'static [&'static str]),
    /// the line contains one of these.
    ContainsAny(&'static [&'static str]),
    /// the line carries at least this many whitespace-separated words.
    MinWords(usize),
    /// the word at this index parses as a `u32`.
    WordIsNumber(usize),
    /// the word at this index is a dotted-decimal subnet or wildcard mask.
    WordIsDottedMask(usize),
    /// the word at this index contains a `/`.
    WordContainsSlash(usize),
    /// some word is a dotted-decimal subnet or wildcard mask.
    AnyWordIsDottedMask,
    /// the word at this index is a well-known Junos top-level stanza name.
    WordIsJunosStanza(usize),
    /// the line is an `interface` line whose name has this shape.
    InterfaceName(NameShape),
    /// the line does not satisfy this test.
    Not(&'static Test),
}

/// the shape of the interface name on an `interface` line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameShape {
    /// the name starts with one of these, case-sensitively.
    StartsWithAny(&'static [&'static str]),
    /// the name contains a `/`, i.e. it carries slot notation.
    ContainsSlash,
    /// the name is an IOS XE speed-prefixed Ethernet name.
    IosxeEthernet,
    /// the name is an IOS XR interface name.
    Iosxr,
}

/// one pattern a vendor's configuration text carries, and what it is worth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signal {
    /// what a matching line adds to this vendor's score, normally one of
    /// [`STRONG_SIGNAL`], [`MODERATE_SIGNAL`] and [`WEAK_SIGNAL`].
    pub weight: i32,
    /// the tests a line must all satisfy to match.
    pub tests: &'static [Test],
}

/// the signals one vendor is scored on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalTable {
    /// the name [`DialectHint::Named`] carries when this vendor wins.
    pub name: &'static str,
    /// every pattern that scores for this vendor.
    pub signals: &'static [Signal],
}

impl Test {
    fn holds(&self, line: &str, words: &[&str]) -> bool {
        match self {
            Test::StartsWithAny(prefixes) => prefixes.iter().any(|p| line.starts_with(p)),
            Test::IsAny(literals) => literals.contains(&line),
            Test::EndsWithAny(suffixes) => suffixes.iter().any(|s| line.ends_with(s)),
            Test::ContainsAny(needles) => needles.iter().any(|n| line.contains(n)),
            Test::MinWords(count) => words.len() >= *count,
            Test::WordIsNumber(index) => {
                words.get(*index).is_some_and(|w| w.parse::<u32>().is_ok())
            }
            Test::WordIsDottedMask(index) => {
                words.get(*index).is_some_and(|w| looks_like_dotted_mask(w))
            }
            Test::WordContainsSlash(index) => words.get(*index).is_some_and(|w| w.contains('/')),
            Test::AnyWordIsDottedMask => words.iter().any(|w| looks_like_dotted_mask(w)),
            Test::WordIsJunosStanza(index) => {
                words.get(*index).is_some_and(|w| is_junos_stanza_name(w))
            }
            Test::InterfaceName(shape) => match interface_name(line) {
                Some(name) => shape.holds(name),
                None => false,
            },
            Test::Not(test) => !test.holds(line, words),
        }
    }
}

impl NameShape {
    fn holds(&self, name: &str) -> bool {
        match self {
            NameShape::StartsWithAny(prefixes) => prefixes.iter().any(|p| name.starts_with(p)),
            NameShape::ContainsSlash => name.contains('/'),
            NameShape::IosxeEthernet => is_iosxe_ethernet_name(name),
            NameShape::Iosxr => is_iosxr_interface_name(name),
        }
    }
}

impl Signal {
    fn matches(&self, line: &str, words: &[&str]) -> bool {
        self.tests.iter().all(|test| test.holds(line, words))
    }
}

/// detect the likely network-device dialect from configuration text.
///
/// `dialects` is the set of vendors in the running, each with the signals its
/// configuration text carries.  returns [`DialectHint::Named`] with the
/// winner's name, or [`DialectHint::Generic`] when no vendor scores high
/// enough or the runner-up is too close.
pub fn detect_dialect(input: &str, dialects: &[SignalTable]) -> DialectHint {
    let lines: Vec<&str> = input.lines().map(str::trim).collect();
    let mut scores = vec![0i32; dialects.len()];

    for (&line, scorable) in lines.iter().zip(scorable_lines(&lines)) {
        if !scorable {
            continue;
        }

        let words: Vec<&str> = line.split_whitespace().collect();
        for (score, table) in scores.iter_mut().zip(dialects) {
            for signal in table.signals {
                if signal.matches(line, &words) {
                    *score += signal.weight;
                }
            }
        }
    }

    let mut ranked: Vec<(&str, i32)> = dialects
        .iter()
        .map(|table| table.name)
        .zip(scores)
        .collect();
    ranked.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));

    let Some(&(best_name, best_score)) = ranked.first() else {
        return DialectHint::Generic;
    };
    let second_score = ranked.get(1).map_or(0, |candidate| candidate.1);

    if best_score < MIN_CONFIDENCE_SCORE {
        return DialectHint::Generic;
    }
    if best_score < second_score * MARGIN_FACTOR {
        return DialectHint::Generic;
    }

    DialectHint::Named(best_name.to_string())
}

/// the interface name on an `interface` line, or `None` for any other line.
fn interface_name(line: &str) -> Option<&str> {
    line.starts_with("interface ")
        .then(|| line.trim_start_matches("interface "))
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
/// crate can check that: the tables live in `netform_dialects`, which depends
/// on this one. its `detect_guard_coverage` suite sees both and asserts it.
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
    fn an_empty_table_set_detects_nothing() {
        assert_eq!(detect_dialect("feature bgp\n", &[]), DialectHint::Generic);
    }

    #[test]
    fn a_lone_table_needs_no_runner_up() {
        const SIGNALS: &[Signal] = &[Signal {
            weight: STRONG_SIGNAL,
            tests: &[Test::StartsWithAny(&["feature "])],
        }];
        let tables = [SignalTable {
            name: "nxos",
            signals: SIGNALS,
        }];
        assert_eq!(
            detect_dialect("feature bgp\n", &tables),
            DialectHint::Named("nxos".into()),
        );
    }

    #[test]
    fn interface_name_strips_every_leading_keyword() {
        assert_eq!(interface_name("interface Ethernet1"), Some("Ethernet1"));
        assert_eq!(
            interface_name("interface interface Ethernet1"),
            Some("Ethernet1"),
        );
        assert_eq!(interface_name("ip address 10.0.0.1/24"), None);
    }
}
