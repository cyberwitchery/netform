use std::fs;
use std::io::{self, IsTerminal, Read as _};
use std::path::{Path, PathBuf};
use std::process;

use clap::{Parser, ValueEnum};
use netform_dialect_eos::parse_eos;
use netform_dialect_fortios::parse_fortios;
use netform_dialect_iosxe::parse_iosxe;
use netform_dialect_junos::parse_junos;
use netform_dialect_nxos::parse_nxos;
use netform_diff::{
    DEFAULT_CONTEXT_LINES, NormalizationStep, NormalizeOptions, OrderPolicy, OrderPolicyConfig,
    build_plan, diff_documents, format_markdown_report, format_unified_diff,
};
use netform_ir::{Document, parse_generic};

#[derive(Debug, Parser)]
#[command(name = "config-diff")]
#[command(about = "Compare two config files and print a drift report")]
struct Cli {
    file_a: PathBuf,
    file_b: PathBuf,

    #[arg(long)]
    json: bool,

    #[arg(long)]
    plan_json: bool,

    #[arg(long)]
    ignore_comments: bool,

    #[arg(long)]
    ignore_blank_lines: bool,

    #[arg(long)]
    normalize_whitespace: bool,

    #[arg(long)]
    trim_trailing_whitespace: bool,

    #[arg(long)]
    normalize_leading_whitespace: bool,

    #[arg(long, value_enum, default_value_t = CliOrderPolicy::Ordered)]
    order_policy: CliOrderPolicy,

    #[arg(long, value_enum, default_value_t = CliDialect::Auto)]
    dialect: CliDialect,

    /// Output format for the human-readable report.
    #[arg(long, value_enum, default_value_t = CliFormat::Unified)]
    format: CliFormat,

    /// Maximum number of lines shown per side of each edit before
    /// truncating with "and N more".  Applies to unified and markdown
    /// output (ignored with --json / --plan-json).
    #[arg(long, default_value_t = DEFAULT_CONTEXT_LINES)]
    context_lines: usize,

    /// Force colored output even when stdout is not a TTY.
    #[arg(long, conflicts_with = "no_color")]
    color: bool,

    /// Disable colored output.
    #[arg(long)]
    no_color: bool,

    /// Suppress exit code 1 when configs differ.  By default config-diff
    /// exits 1 when the configs differ (like `diff(1)`).  Pass this flag
    /// to exit 0 instead.  Exit code 2 (I/O or serialization error) is
    /// never suppressed.
    #[arg(long)]
    no_exit_code: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliFormat {
    Unified,
    Markdown,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliOrderPolicy {
    Ordered,
    Unordered,
    KeyedStable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliDialect {
    Auto,
    Generic,
    Eos,
    Fortios,
    Iosxe,
    Junos,
    Nxos,
}

fn main() {
    let cli = Cli::parse();

    let is_a_stdin = cli.file_a.as_os_str() == "-";
    let is_b_stdin = cli.file_b.as_os_str() == "-";

    let (a_text, a_label, b_text, b_label) = if is_a_stdin && is_b_stdin {
        let (text, label) = read_input(&cli.file_a);
        (text.clone(), label.clone(), text, label)
    } else {
        let (a_text, a_label) = read_input(&cli.file_a);
        let (b_text, b_label) = read_input(&cli.file_b);
        (a_text, a_label, b_text, b_label)
    };

    let resolved_dialect = match cli.dialect {
        CliDialect::Auto => {
            let a_detected = detect_dialect(&a_text);
            let b_detected = detect_dialect(&b_text);
            if a_detected == b_detected {
                a_detected
            } else {
                // Disagreement: fall back to Generic rather than risk
                // parsing two files with different grammars.
                CliDialect::Generic
            }
        }
        other => other,
    };

    let a_doc = parse_config(&a_text, resolved_dialect);
    let b_doc = parse_config(&b_text, resolved_dialect);

    let mut steps = Vec::new();
    if cli.ignore_comments {
        steps.push(NormalizationStep::IgnoreComments);
    }
    if cli.ignore_blank_lines {
        steps.push(NormalizationStep::IgnoreBlankLines);
    }
    if cli.normalize_whitespace {
        steps.push(NormalizationStep::CollapseInternalWhitespace);
    }
    if cli.trim_trailing_whitespace {
        steps.push(NormalizationStep::TrimTrailingWhitespace);
    }
    if cli.normalize_leading_whitespace {
        steps.push(NormalizationStep::NormalizeLeadingWhitespace);
    }
    let policy = match cli.order_policy {
        CliOrderPolicy::Ordered => OrderPolicy::Ordered,
        CliOrderPolicy::Unordered => OrderPolicy::Unordered,
        CliOrderPolicy::KeyedStable => OrderPolicy::KeyedStable,
    };
    let options = NormalizeOptions::new(steps).with_order_policy(OrderPolicyConfig {
        default: policy,
        overrides: Vec::new(),
    });

    let diff = diff_documents(&a_doc, &b_doc, options);

    let use_color = if cli.color {
        true
    } else if cli.no_color {
        false
    } else {
        std::io::stdout().is_terminal()
    };
    owo_colors::set_override(use_color);

    if cli.plan_json {
        let plan = build_plan(&diff);
        match serde_json::to_string_pretty(&plan) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("config-diff: {e}");
                process::exit(2);
            }
        }
    } else if cli.json {
        match serde_json::to_string_pretty(&diff) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("config-diff: {e}");
                process::exit(2);
            }
        }
    } else {
        let output = match cli.format {
            CliFormat::Unified => format_unified_diff(&diff, &a_label, &b_label, cli.context_lines),
            CliFormat::Markdown => {
                format_markdown_report(&diff, &a_label, &b_label, cli.context_lines)
            }
        };
        print!("{output}");
    }

    if !cli.no_exit_code && diff.has_changes {
        process::exit(1);
    }
}

fn read_input(path: &Path) -> (String, String) {
    if path.as_os_str() == "-" {
        let mut buf = String::new();
        match io::stdin().read_to_string(&mut buf) {
            Ok(_) => (buf, "<stdin>".to_string()),
            Err(e) => {
                eprintln!("config-diff: <stdin>: {e}");
                process::exit(2);
            }
        }
    } else {
        match fs::read_to_string(path) {
            Ok(s) => (s, path.display().to_string()),
            Err(e) => {
                eprintln!("config-diff: {}: {e}", path.display());
                process::exit(2);
            }
        }
    }
}

fn parse_config(input: &str, dialect: CliDialect) -> Document {
    let resolved = match dialect {
        CliDialect::Auto => detect_dialect(input),
        other => other,
    };
    match resolved {
        CliDialect::Auto => unreachable!(),
        CliDialect::Generic => parse_generic(input),
        CliDialect::Eos => parse_eos(input),
        CliDialect::Fortios => parse_fortios(input),
        CliDialect::Iosxe => parse_iosxe(input),
        CliDialect::Junos => parse_junos(input),
        CliDialect::Nxos => parse_nxos(input),
    }
}

/// Score-based dialect detection from config content.
///
/// Scans lines for dialect-specific patterns, accumulates scores, and returns
/// the highest-scoring dialect.  Falls back to `Generic` when the top score is
/// zero or when the margin between the top two candidates is too narrow.
fn detect_dialect(input: &str) -> CliDialect {
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
        (CliDialect::Fortios, fortios),
        (CliDialect::Junos, junos),
        (CliDialect::Nxos, nxos),
        (CliDialect::Eos, eos),
        (CliDialect::Iosxe, iosxe),
    ];

    let mut sorted = candidates;
    sorted.sort_by_key(|c| std::cmp::Reverse(c.1));

    let (best_dialect, best_score) = sorted[0];
    let (_, second_score) = sorted[1];

    // Require a minimum score and a clear margin over the runner-up.
    if best_score < 3 {
        return CliDialect::Generic;
    }
    if best_score > 0 && second_score > 0 && best_score < second_score * 2 {
        return CliDialect::Generic;
    }

    best_dialect
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
        assert_eq!(detect_dialect(input), CliDialect::Fortios);
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
        assert_eq!(detect_dialect(input), CliDialect::Junos);
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
        assert_eq!(detect_dialect(input), CliDialect::Junos);
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
        assert_eq!(detect_dialect(input), CliDialect::Nxos);
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
        assert_eq!(detect_dialect(input), CliDialect::Eos);
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
        assert_eq!(detect_dialect(input), CliDialect::Iosxe);
    }

    #[test]
    fn detect_generic_for_empty() {
        assert_eq!(detect_dialect(""), CliDialect::Generic);
    }

    #[test]
    fn detect_generic_for_plain_text() {
        let input = "\
hostname router
# a comment
some random config line
";
        assert_eq!(detect_dialect(input), CliDialect::Generic);
    }

    #[test]
    fn detect_generic_when_ambiguous() {
        // Minimal content with weak signals from multiple dialects.
        let input = "\
set hostname myrouter
interface Ethernet1
";
        // Both junos/fortios and eos get mild scores — should fall back.
        assert_eq!(detect_dialect(input), CliDialect::Generic);
    }
}
