use std::fs;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use netform_dialect_eos::parse_eos;
use netform_dialect_iosxe::parse_iosxe;
use netform_dialect_junos::parse_junos;
use netform_diff::{
    NormalizationStep, NormalizeOptions, OrderPolicy, OrderPolicyConfig, build_plan,
    diff_documents, format_markdown_report,
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

    #[arg(long, value_enum, default_value_t = CliOrderPolicy::Ordered)]
    order_policy: CliOrderPolicy,

    #[arg(long, value_enum, default_value_t = CliDialect::Generic)]
    dialect: CliDialect,
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
    Iosxe,
    Junos,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let a_text = fs::read_to_string(&cli.file_a)?;
    let b_text = fs::read_to_string(&cli.file_b)?;

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

    if cli.plan_json {
        let plan = build_plan(&diff);
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else if cli.json {
        println!("{}", serde_json::to_string_pretty(&diff)?);
    } else {
        println!(
            "{}",
            format_markdown_report(
                &diff,
                &cli.file_a.display().to_string(),
                &cli.file_b.display().to_string(),
            )
        );
    }

    Ok(())
}

fn parse_config(input: &str, dialect: CliDialect) -> Document {
    match dialect {
        CliDialect::Generic => parse_generic(input),
        CliDialect::Eos => parse_eos(input),
        CliDialect::Iosxe => parse_iosxe(input),
        CliDialect::Junos => parse_junos(input),
    }
}
