use std::fs;
use std::io::{self, IsTerminal, Read as _};
use std::path::{Path, PathBuf};
use std::process;

use clap::builder::TypedValueParser as _;
use clap::{Parser, ValueEnum};
use netform_dialects::DialectEntry;
use netform_diff::{
    ColorChoice, DEFAULT_CONTEXT_LINES, NormalizationStep, NormalizeOptions, OrderPolicy,
    OrderPolicyConfig, OrderPolicyOverride, build_plan, diff_documents, format_markdown_report,
    format_unified_diff_with_color,
};
use netform_ir::{DialectHint, Document, parse_generic};

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

    /// per-context order-policy override.  format: PATH:POLICY where PATH
    /// is a dot-separated context prefix (e.g. "0.1") and POLICY is one of
    /// ordered, unordered, or keyed-stable.  may be repeated.
    #[arg(long, value_parser = parse_policy_override)]
    policy_override: Vec<OrderPolicyOverride>,

    /// parser profile: `auto` detects it from the file contents and falls
    /// back to `generic` when the two files disagree.
    #[arg(long, default_value = "auto", value_parser = dialect_value_parser())]
    dialect: CliDialect,

    /// output format for the human-readable report.
    #[arg(long, value_enum, default_value_t = CliFormat::Unified)]
    format: CliFormat,

    /// maximum number of lines shown per side of each edit before
    /// truncating with "and N more".  applies to unified and markdown
    /// output (ignored with --json / --plan-json).
    #[arg(long, default_value_t = DEFAULT_CONTEXT_LINES)]
    context_lines: usize,

    /// force colored output even when stdout is not a TTY or `NO_COLOR`
    /// is set.
    #[arg(long, conflicts_with = "no_color")]
    color: bool,

    /// disable colored output.  colors are off by default when stdout is
    /// not a TTY, or when the `NO_COLOR` environment variable is set to a
    /// non-empty value.
    #[arg(long)]
    no_color: bool,

    /// suppress exit code 1 when configs differ.  by default config-diff
    /// exits 1 when the configs differ (like `diff(1)`).  pass this flag
    /// to exit 0 instead.  exit code 2 (I/O or serialization error) is
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliDialect {
    Auto,
    Generic,
    Vendor(&'static DialectEntry),
}

impl CliDialect {
    fn from_name(name: &str) -> Self {
        match name {
            "auto" => CliDialect::Auto,
            "generic" => CliDialect::Generic,
            _ => netform_dialects::find(name).map_or(CliDialect::Generic, CliDialect::Vendor),
        }
    }

    fn from_hint(hint: &DialectHint) -> Self {
        match hint {
            DialectHint::Named(name) => CliDialect::from_name(name),
            _ => CliDialect::Generic,
        }
    }
}

/// accept `auto`, `generic`, and every vendor in `netform_dialects::REGISTRY`.
fn dialect_value_parser() -> impl clap::builder::TypedValueParser<Value = CliDialect> {
    let choices: Vec<&'static str> = ["auto", "generic"]
        .into_iter()
        .chain(netform_dialects::names())
        .collect();
    clap::builder::PossibleValuesParser::new(choices).map(|name| CliDialect::from_name(&name))
}

fn parse_policy_override(s: &str) -> Result<OrderPolicyOverride, String> {
    let (path_str, policy_str) = s
        .split_once(':')
        .ok_or_else(|| format!("expected PATH:POLICY (e.g. \"0.1:unordered\"), got \"{s}\""))?;

    let context_prefix: Vec<usize> = if path_str.is_empty() {
        Vec::new()
    } else {
        path_str
            .split('.')
            .map(|seg| {
                seg.parse::<usize>()
                    .map_err(|_| format!("invalid path segment \"{seg}\" — expected an integer"))
            })
            .collect::<Result<_, _>>()?
    };

    let policy = match policy_str {
        "ordered" => OrderPolicy::Ordered,
        "unordered" => OrderPolicy::Unordered,
        "keyed-stable" => OrderPolicy::KeyedStable,
        other => {
            return Err(format!(
                "unknown policy \"{other}\" — expected ordered, unordered, or keyed-stable"
            ));
        }
    };

    Ok(OrderPolicyOverride {
        context_prefix,
        policy,
    })
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
            let a_hint = netform_ir::detect::detect_dialect(&a_text);
            let b_hint = netform_ir::detect::detect_dialect(&b_text);
            if a_hint == b_hint {
                CliDialect::from_hint(&a_hint)
            } else {
                // disagreement: fall back to Generic rather than risk
                // parsing two files with different grammars.
                eprintln!(
                    "config-diff: warning: auto-detected dialects disagree ({} vs {}), \
                     falling back to generic (use --dialect to override)",
                    hint_label(&a_hint),
                    hint_label(&b_hint),
                );
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
        overrides: cli.policy_override,
    });

    let diff = match diff_documents(&a_doc, &b_doc, options) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("config-diff: {e}");
            process::exit(2);
        }
    };

    let color = resolve_color(
        cli.color,
        cli.no_color,
        no_color_env(),
        io::stdout().is_terminal(),
    );

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
            CliFormat::Unified => {
                format_unified_diff_with_color(&diff, &a_label, &b_label, cli.context_lines, color)
            }
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

/// `NO_COLOR` disables color when set to any non-empty value (no-color.org).
fn no_color_env() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

fn resolve_color(
    force_color: bool,
    no_color: bool,
    no_color_env: bool,
    stdout_is_terminal: bool,
) -> ColorChoice {
    if force_color {
        ColorChoice::Always
    } else if no_color || no_color_env {
        ColorChoice::Never
    } else if stdout_is_terminal {
        ColorChoice::Always
    } else {
        ColorChoice::Never
    }
}

fn hint_label(hint: &DialectHint) -> &str {
    match hint {
        DialectHint::Named(name) => name.as_str(),
        DialectHint::Generic => "generic",
        DialectHint::Unknown => "unknown",
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

/// parse input with automatic dialect detection and full dialect dispatch.
///
/// runs dialect detection on the input and dispatches to the appropriate
/// dialect-specific parser (Junos, FortiOS, EOS, IOS XE, IOS XR, NX-OS) so that
/// trivia classification, tokenization, and key hints are dialect-aware.
/// falls back to the generic parser when no dialect is detected with
/// sufficient confidence.
#[cfg(test)]
fn auto_parse(input: &str) -> Document {
    parse_config(input, CliDialect::Auto)
}

fn parse_config(input: &str, dialect: CliDialect) -> Document {
    let resolved = match dialect {
        CliDialect::Auto => CliDialect::from_hint(&netform_ir::detect::detect_dialect(input)),
        other => other,
    };
    match resolved {
        CliDialect::Auto => unreachable!(),
        CliDialect::Generic => parse_generic(input),
        CliDialect::Vendor(entry) => (entry.parse)(input),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::Node;

    /// helper: return the key hint of the first root node's header (if block)
    /// or the node itself (if line).
    fn first_root_key_hint(doc: &Document) -> Option<String> {
        let id = *doc.roots.first()?;
        match doc.node(id)? {
            Node::Block(b) => b.header.key_hint.clone(),
            Node::Line(l) => l.key_hint.clone(),
        }
    }

    #[test]
    fn auto_parse_junos_produces_dialect_key_hints() {
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
        // junos parser sets key_hint = "interfaces"; generic parser would not.
        assert_eq!(first_root_key_hint(&doc), Some("interfaces".into()));
    }

    #[test]
    fn auto_parse_nxos_produces_dialect_key_hints() {
        let input = "\
feature bgp
feature interface-vlan
feature lacp
interface Ethernet1/1
  description uplink
  no shutdown
";
        let doc = auto_parse(input);
        assert_eq!(doc.metadata.dialect_hint, DialectHint::Named("nxos".into()),);
        // nxos parser sets key_hint = "feature:bgp"; generic parser would not.
        assert_eq!(first_root_key_hint(&doc), Some("feature:bgp".into()));
    }

    #[test]
    fn auto_parse_eos_produces_dialect_key_hints() {
        let input = "\
interface Ethernet1
   description uplink-spine-a
   mtu 9214
   ip address 192.0.2.2/31
   no shutdown
ip access-list ACL-EDGE-IN
   10 permit tcp 10.10.1.0/24 any eq https
   20 permit tcp 10.10.1.0/24 any eq ssh
   90 deny ip any any log
";
        let doc = auto_parse(input);
        assert_eq!(doc.metadata.dialect_hint, DialectHint::Named("eos".into()),);
        // eos parser sets key_hint = "interface:ethernet:1"; generic would not.
        assert!(first_root_key_hint(&doc).is_some());
    }

    #[test]
    fn auto_parse_generic_fallback() {
        let doc = auto_parse("hostname router\n");
        assert_eq!(doc.metadata.dialect_hint, DialectHint::Generic);
        // generic parser does not produce key hints.
        assert_eq!(first_root_key_hint(&doc), None);
    }

    #[test]
    fn auto_parse_preserves_content() {
        let input = "\
interfaces {
    ge-0/0/0 {
        mtu 9216;
    }
}
";
        assert_eq!(auto_parse(input).render(), input);
    }

    #[test]
    fn parse_policy_override_simple() {
        let result = parse_policy_override("0:unordered").unwrap();
        assert_eq!(result.context_prefix, vec![0]);
        assert_eq!(result.policy, OrderPolicy::Unordered);
    }

    #[test]
    fn parse_policy_override_dotted_path() {
        let result = parse_policy_override("0.1.2:keyed-stable").unwrap();
        assert_eq!(result.context_prefix, vec![0, 1, 2]);
        assert_eq!(result.policy, OrderPolicy::KeyedStable);
    }

    #[test]
    fn parse_policy_override_empty_path() {
        let result = parse_policy_override(":ordered").unwrap();
        assert_eq!(result.context_prefix, Vec::<usize>::new());
        assert_eq!(result.policy, OrderPolicy::Ordered);
    }

    #[test]
    fn parse_policy_override_missing_colon() {
        assert!(parse_policy_override("0-unordered").is_err());
    }

    #[test]
    fn parse_policy_override_bad_segment() {
        assert!(parse_policy_override("0.abc:unordered").is_err());
    }

    #[test]
    fn parse_policy_override_bad_policy() {
        assert!(parse_policy_override("0:bogus").is_err());
    }

    #[test]
    fn color_flags_beat_the_terminal_check() {
        assert_eq!(
            resolve_color(true, false, false, false),
            ColorChoice::Always
        );
        assert_eq!(resolve_color(false, true, false, true), ColorChoice::Never);
    }

    #[test]
    fn without_flags_the_terminal_check_decides() {
        assert_eq!(
            resolve_color(false, false, false, true),
            ColorChoice::Always
        );
        assert_eq!(
            resolve_color(false, false, false, false),
            ColorChoice::Never
        );
    }

    #[test]
    fn no_color_env_disables_color_on_a_terminal() {
        assert_eq!(resolve_color(false, false, true, true), ColorChoice::Never);
    }

    #[test]
    fn forcing_color_beats_no_color_env() {
        assert_eq!(resolve_color(true, false, true, true), ColorChoice::Always);
        assert_eq!(resolve_color(true, false, true, false), ColorChoice::Always);
    }
}
