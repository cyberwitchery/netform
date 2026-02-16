use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_file_path(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("netform-{prefix}-{nonce}.cfg"))
}

#[test]
fn config_diff_cli_prints_markdown_report() {
    let left = temp_file_path("left-markdown");
    let right = temp_file_path("right-markdown");
    fs::write(&left, "hostname old\n").expect("write left");
    fs::write(&right, "hostname new\n").expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("# Config Diff Report"));
    assert!(stdout.contains("Replaces:"));
}

#[test]
fn config_diff_cli_emits_json_and_plan_json() {
    let left = temp_file_path("left-json");
    let right = temp_file_path("right-json");
    fs::write(&left, "interface Ethernet1\n  description old\n").expect("write left");
    fs::write(&right, "interface Ethernet1\n  description new\n").expect("write right");

    let diff_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --json");
    assert!(diff_output.status.success());
    let diff_json: serde_json::Value =
        serde_json::from_slice(&diff_output.stdout).expect("valid diff json");
    assert_eq!(diff_json["has_changes"], true);
    assert!(diff_json.get("edits").is_some());

    let plan_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--plan-json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --plan-json");
    assert!(plan_output.status.success());
    let plan_json: serde_json::Value =
        serde_json::from_slice(&plan_output.stdout).expect("valid plan json");
    assert!(plan_json.get("actions").is_some());
    assert!(plan_json.get("version").is_some());
}

#[test]
fn config_diff_cli_accepts_dialect_flag() {
    let left = temp_file_path("left-dialect");
    let right = temp_file_path("right-dialect");
    fs::write(
        &left,
        "interfaces {\n    ge-0/0/0 {\n        description \"a\";\n    }\n}\n",
    )
    .expect("write left");
    fs::write(
        &right,
        "interfaces {\n    ge-0/0/0 {\n        description \"b\";\n    }\n}\n",
    )
    .expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--dialect")
        .arg("junos")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --dialect junos");

    assert!(output.status.success());
    let diff_json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid json");
    assert_eq!(diff_json["has_changes"], true);
}

#[test]
fn config_diff_cli_fails_for_missing_file() {
    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("/definitely/missing-left.cfg")
        .arg("/definitely/missing-right.cfg")
        .output()
        .expect("run config-diff");

    assert!(!output.status.success());
}

#[test]
fn replay_fixtures_cli_runs_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_netform-replay-fixtures"))
        .output()
        .expect("run replay binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("replayed"));
    assert!(stdout.contains("fixture"));
}
