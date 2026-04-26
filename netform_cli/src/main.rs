use std::fs;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process;

use clap::{Parser, ValueEnum};
use netform_dialect_eos::parse_eos;
use netform_dialect_fortios::parse_fortios;
use netform_dialect_iosxe::parse_iosxe;
use netform_dialect_junos::parse_junos;
use netform_dialect_nxos::parse_nxos;
use netform_diff::{
    DEFAULT_CONTEXT_LINES, NormalizationStep, NormalizeOptions, OrderPolicy, OrderPolicyConfig,
    build_plan, diff_documents, format_unified_diff,
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

    #[arg(long, value_enum, default_value_t = CliDialect::Generic)]
    dialect: CliDialect,

    /// Maximum number of lines shown per side of each edit before
    /// truncating with "and N more".  Applies to the unified-diff
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
enum CliOrderPolicy {
    Ordered,
    Unordered,
    KeyedStable,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliDialect {
    Generic,
    Eos,
    Fortios,
    Iosxe,
    Junos,
    Nxos,
}

fn main() {
    let cli = Cli::parse();

    let a_text = match fs::read_to_string(&cli.file_a) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("config-diff: {}: {e}", cli.file_a.display());
            process::exit(2);
        }
    };
    let b_text = match fs::read_to_string(&cli.file_b) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("config-diff: {}: {e}", cli.file_b.display());
            process::exit(2);
        }
    };

    let a_doc = parse_config(&a_text, cli.dialect);
    let b_doc = parse_config(&b_text, cli.dialect);

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
        print!(
            "{}",
            format_unified_diff(
                &diff,
                &cli.file_a.display().to_string(),
                &cli.file_b.display().to_string(),
                cli.context_lines,
            )
        );
    }

    if !cli.no_exit_code && diff.has_changes {
        process::exit(1);
    }
}

fn parse_config(input: &str, dialect: CliDialect) -> Document {
    match dialect {
        CliDialect::Generic => parse_generic(input),
        CliDialect::Eos => parse_eos(input),
        CliDialect::Fortios => parse_fortios(input),
        CliDialect::Iosxe => parse_iosxe(input),
        CliDialect::Junos => parse_junos(input),
        CliDialect::Nxos => parse_nxos(input),
    }
}
