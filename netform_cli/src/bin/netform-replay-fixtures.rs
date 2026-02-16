use std::fs;
use std::path::Path;

use netform_dialect_eos::parse_eos;
use netform_dialect_iosxe::parse_iosxe;
use netform_dialect_junos::parse_junos;
use netform_diff::{NormalizeOptions, OrderPolicyConfig, diff_documents};
use netform_ir::{Document, parse_generic};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    #[serde(default)]
    dialect: FixtureDialect,
    intended: String,
    actual: String,
    normalization_steps: Vec<netform_diff::NormalizationStep>,
    order_policy: OrderPolicyConfig,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
struct Expected {
    has_changes: bool,
    edit_types: Vec<String>,
    finding_codes: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum FixtureDialect {
    #[default]
    Generic,
    Eos,
    Iosxe,
    Junos,
}

fn edit_type_name(edit: &netform_diff::Edit) -> &'static str {
    match edit {
        netform_diff::Edit::Insert { .. } => "Insert",
        netform_diff::Edit::Delete { .. } => "Delete",
        netform_diff::Edit::Replace { .. } => "Replace",
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let fixtures_dir = repo_root.join("fixtures");

    let mut entries = fs::read_dir(&fixtures_dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.path());

    let mut checked = 0usize;
    for entry in entries {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        let raw = fs::read_to_string(&path)?;
        let fixture: Fixture = serde_json::from_str(&raw)?;

        let intended = parse_config(&fixture.intended, fixture.dialect);
        let actual = parse_config(&fixture.actual, fixture.dialect);

        let options = NormalizeOptions::new(fixture.normalization_steps)
            .with_order_policy(fixture.order_policy);
        let diff = diff_documents(&intended, &actual, options);

        if diff.has_changes != fixture.expected.has_changes {
            return Err(format!(
                "fixture {}: has_changes mismatch: expected {}, got {}",
                fixture.name, fixture.expected.has_changes, diff.has_changes
            )
            .into());
        }

        let edit_types = diff
            .edits
            .iter()
            .map(edit_type_name)
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if edit_types != fixture.expected.edit_types {
            return Err(format!(
                "fixture {}: edit_types mismatch: expected {:?}, got {:?}",
                fixture.name, fixture.expected.edit_types, edit_types
            )
            .into());
        }

        let finding_codes = diff
            .findings
            .iter()
            .map(|f| f.code.clone())
            .collect::<Vec<_>>();
        if finding_codes != fixture.expected.finding_codes {
            return Err(format!(
                "fixture {}: finding_codes mismatch: expected {:?}, got {:?}",
                fixture.name, fixture.expected.finding_codes, finding_codes
            )
            .into());
        }

        checked += 1;
    }

    println!("replayed {checked} fixture(s)");
    Ok(())
}

fn parse_config(input: &str, dialect: FixtureDialect) -> Document {
    match dialect {
        FixtureDialect::Generic => parse_generic(input),
        FixtureDialect::Eos => parse_eos(input),
        FixtureDialect::Iosxe => parse_iosxe(input),
        FixtureDialect::Junos => parse_junos(input),
    }
}
