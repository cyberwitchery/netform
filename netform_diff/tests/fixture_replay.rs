use std::fs;
use std::path::Path;

use netform_diff::{NormalizationStep, NormalizeOptions, OrderPolicyConfig, diff_documents};
use serde_json::Value;

#[derive(serde::Deserialize)]
struct Fixture {
    name: String,
    #[serde(default = "generic")]
    dialect: String,
    intended: String,
    actual: String,
    normalization_steps: Vec<NormalizationStep>,
    order_policy: OrderPolicyConfig,
    expected: Expected,
}

#[derive(serde::Deserialize)]
struct Expected {
    has_changes: bool,
    edits: Value,
    finding_codes: Vec<String>,
}

fn generic() -> String {
    netform_dialects::GENERIC.name.to_string()
}

#[test]
fn every_fixture_replays_to_its_expected_diff() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures");
    let mut paths = fs::read_dir(fixtures)
        .expect("read fixtures")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures found");

    for path in paths {
        let fixture: Fixture =
            serde_json::from_str(&fs::read_to_string(&path).expect("read fixture"))
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        let parse = netform_dialects::find(&fixture.dialect)
            .unwrap_or_else(|| panic!("{}: unknown dialect {}", fixture.name, fixture.dialect))
            .parse;
        let options = NormalizeOptions::new(fixture.normalization_steps)
            .with_order_policy(fixture.order_policy);
        let diff = diff_documents(&parse(&fixture.intended), &parse(&fixture.actual), options)
            .unwrap_or_else(|err| panic!("{}: {err}", fixture.name));

        let edits = serde_json::to_value(&diff.edits).expect("serialize edits");
        let finding_codes = diff
            .findings
            .iter()
            .map(|finding| finding.code.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            diff.has_changes, fixture.expected.has_changes,
            "{}: has_changes",
            fixture.name
        );
        assert!(
            edits == fixture.expected.edits,
            "{}: edits differ, got:\n{}",
            fixture.name,
            serde_json::to_string_pretty(&diff.edits).expect("serialize edits")
        );
        assert_eq!(
            finding_codes, fixture.expected.finding_codes,
            "{}: finding codes",
            fixture.name
        );
    }
}
