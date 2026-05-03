use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_file_path(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("netform-{prefix}-{nonce}.cfg"))
}

#[test]
fn config_diff_cli_prints_unified_diff() {
    let left = temp_file_path("left-unified");
    let right = temp_file_path("right-unified");
    fs::write(&left, "hostname old\n").expect("write left");
    fs::write(&right, "hostname new\n").expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("---"), "should contain --- header");
    assert!(stdout.contains("+++"), "should contain +++ header");
    assert!(stdout.contains("@@"), "should contain @@ hunk header");
    assert!(
        stdout.contains("- hostname old"),
        "should contain deleted line"
    );
    assert!(
        stdout.contains("+ hostname new"),
        "should contain inserted line"
    );
}

#[test]
fn config_diff_cli_emits_json_and_plan_json() {
    let left = temp_file_path("left-json");
    let right = temp_file_path("right-json");
    fs::write(&left, "interface Ethernet1\n  description old\n").expect("write left");
    fs::write(&right, "interface Ethernet1\n  description new\n").expect("write right");

    let diff_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
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
        .arg("--no-exit-code")
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
        .arg("--no-exit-code")
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
fn config_diff_cli_exits_two_for_missing_file() {
    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("/definitely/missing-left.cfg")
        .arg("/definitely/missing-right.cfg")
        .output()
        .expect("run config-diff");

    assert_eq!(output.status.code(), Some(2), "I/O error → exit 2");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("missing-left.cfg"),
        "stderr should name the missing file"
    );
}

#[test]
fn config_diff_no_exit_code_does_not_suppress_exit_two() {
    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("/definitely/missing-left.cfg")
        .arg("/definitely/missing-right.cfg")
        .output()
        .expect("run config-diff --no-exit-code");

    assert_eq!(
        output.status.code(),
        Some(2),
        "--no-exit-code must not suppress exit 2 (error)"
    );
}

#[test]
fn config_diff_exits_zero_when_no_changes() {
    let path = temp_file_path("exit-code-same");
    fs::write(&path, "hostname router\n").expect("write file");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg(&path)
        .arg(&path)
        .output()
        .expect("run config-diff");

    assert_eq!(output.status.code(), Some(0), "identical files → exit 0");
}

#[test]
fn config_diff_exits_one_when_changes_detected() {
    let left = temp_file_path("exit-code-left");
    let right = temp_file_path("exit-code-right");
    fs::write(&left, "hostname old\n").expect("write left");
    fs::write(&right, "hostname new\n").expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff");

    assert_eq!(output.status.code(), Some(1), "differing files → exit 1");
}

#[test]
fn config_diff_no_exit_code_suppresses_exit_one() {
    let left = temp_file_path("no-exit-code-left");
    let right = temp_file_path("no-exit-code-right");
    fs::write(&left, "hostname old\n").expect("write left");
    fs::write(&right, "hostname new\n").expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --no-exit-code");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--no-exit-code suppresses exit 1"
    );
}

#[test]
fn config_diff_unordered_policy_ignores_reordered_siblings() {
    let left = temp_file_path("left-unordered");
    let right = temp_file_path("right-unordered");
    fs::write(
        &left,
        "router bgp 65000\n  neighbor 10.0.0.1\n  neighbor 10.0.0.2\n",
    )
    .expect("write left");
    fs::write(
        &right,
        "router bgp 65000\n  neighbor 10.0.0.2\n  neighbor 10.0.0.1\n",
    )
    .expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--order-policy")
        .arg("unordered")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --order-policy unordered");

    assert!(output.status.success());
    let diff_json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid json");
    assert_eq!(
        diff_json["has_changes"], false,
        "unordered policy should ignore sibling reordering"
    );
}

#[test]
fn config_diff_keyed_stable_policy_ignores_reordered_children() {
    let left = temp_file_path("left-keyed-stable");
    let right = temp_file_path("right-keyed-stable");
    fs::write(
        &left,
        "interface Ethernet1\n  description uplink\n  mtu 9000\n",
    )
    .expect("write left");
    fs::write(
        &right,
        "interface Ethernet1\n  mtu 9000\n  description uplink\n",
    )
    .expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--order-policy")
        .arg("keyed-stable")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --order-policy keyed-stable");

    assert!(output.status.success());
    let diff_json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid json");
    assert_eq!(
        diff_json["has_changes"], false,
        "keyed-stable policy should ignore reordered block children"
    );
}

#[test]
fn config_diff_junos_dialect_with_keyed_stable_policy() {
    let left = temp_file_path("left-junos-keyed");
    let right = temp_file_path("right-junos-keyed");
    fs::write(
        &left,
        "interfaces {\n    ge-0/0/0 {\n        description \"uplink\";\n        mtu 9000;\n    }\n}\n",
    )
    .expect("write left");
    fs::write(
        &right,
        "interfaces {\n    ge-0/0/0 {\n        mtu 9000;\n        description \"uplink\";\n    }\n}\n",
    )
    .expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--dialect")
        .arg("junos")
        .arg("--order-policy")
        .arg("keyed-stable")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --dialect junos --order-policy keyed-stable");

    assert!(output.status.success());
    let diff_json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid json");
    assert_eq!(
        diff_json["has_changes"], false,
        "junos + keyed-stable should ignore reordered children within a keyed block"
    );
}

#[test]
fn config_diff_nxos_dialect_produces_diff() {
    let left = temp_file_path("left-nxos");
    let right = temp_file_path("right-nxos");
    fs::write(
        &left,
        "feature ospf\ninterface Ethernet1/1\n  description old-uplink\n  no shutdown\n",
    )
    .expect("write left");
    fs::write(
        &right,
        "feature ospf\ninterface Ethernet1/1\n  description new-uplink\n  no shutdown\n",
    )
    .expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--dialect")
        .arg("nxos")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --dialect nxos");

    assert!(output.status.success());
    let diff_json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid nxos json");
    assert_eq!(diff_json["has_changes"], true);
}

#[test]
fn config_diff_nxos_dialect_no_changes() {
    let path = temp_file_path("nxos-same");
    fs::write(
        &path,
        "feature bgp\nvpc domain 10\n  peer-keepalive destination 10.0.0.1\n",
    )
    .expect("write file");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--dialect")
        .arg("nxos")
        .arg(&path)
        .arg(&path)
        .output()
        .expect("run config-diff --dialect nxos");

    assert_eq!(
        output.status.code(),
        Some(0),
        "identical nxos files → exit 0"
    );
}

#[test]
fn config_diff_nxos_keyed_stable_matches_interfaces() {
    let left = temp_file_path("left-nxos-keyed");
    let right = temp_file_path("right-nxos-keyed");
    fs::write(
        &left,
        "interface Ethernet1/1\n  description uplink\n  mtu 9000\n",
    )
    .expect("write left");
    fs::write(
        &right,
        "interface Ethernet1/1\n  mtu 9000\n  description uplink\n",
    )
    .expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--dialect")
        .arg("nxos")
        .arg("--order-policy")
        .arg("keyed-stable")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --dialect nxos --order-policy keyed-stable");

    assert!(output.status.success());
    let diff_json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid json");
    assert_eq!(
        diff_json["has_changes"], false,
        "nxos + keyed-stable should ignore reordered children"
    );
}

#[test]
fn config_diff_fortios_dialect_produces_diff() {
    let left = temp_file_path("left-fortios");
    let right = temp_file_path("right-fortios");
    // Use structural change (add a line) to produce a diff — FortiOS leaf-line
    // hints stabilize content keys across value changes, so changing a set value
    // alone does not produce edits in ordered mode.
    fs::write(
        &left,
        "config system global\n    set hostname \"FGT\"\nend\n",
    )
    .expect("write left");
    fs::write(
        &right,
        "config system global\n    set hostname \"FGT\"\n    set timezone 04\nend\n",
    )
    .expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--dialect")
        .arg("fortios")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --dialect fortios");

    assert!(output.status.success());
    let diff_json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid fortios json");
    assert_eq!(diff_json["has_changes"], true);
}

#[test]
fn config_diff_fortios_dialect_no_changes() {
    let path = temp_file_path("fortios-same");
    fs::write(
        &path,
        "config firewall address\n    edit \"all\"\n        set type ipmask\n    next\nend\n",
    )
    .expect("write file");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--dialect")
        .arg("fortios")
        .arg(&path)
        .arg(&path)
        .output()
        .expect("run config-diff --dialect fortios");

    assert_eq!(
        output.status.code(),
        Some(0),
        "identical fortios files → exit 0"
    );
}

#[test]
fn config_diff_fortios_unified_output() {
    let left = temp_file_path("left-fortios-unified");
    let right = temp_file_path("right-fortios-unified");
    // Structural change (remove a line) to produce unified diff output.
    fs::write(
        &left,
        "config system global\n    set hostname \"FGT\"\n    set timezone 04\nend\n",
    )
    .expect("write left");
    fs::write(
        &right,
        "config system global\n    set hostname \"FGT\"\nend\n",
    )
    .expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--dialect")
        .arg("fortios")
        .arg("--no-color")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --dialect fortios unified");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("---"), "should contain --- header");
    assert!(stdout.contains("+++"), "should contain +++ header");
    assert!(stdout.contains("@@"), "should contain @@ hunk header");
}

#[test]
fn config_diff_format_markdown_produces_markdown_report() {
    let left = temp_file_path("left-md");
    let right = temp_file_path("right-md");
    fs::write(&left, "hostname old\n").expect("write left");
    fs::write(&right, "hostname new\n").expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--format")
        .arg("markdown")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --format markdown");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("# Config Diff Report"),
        "should contain markdown heading"
    );
    assert!(stdout.contains("## Stats"), "should contain stats section");
    assert!(stdout.contains("## Edits"), "should contain edits section");
}

#[test]
fn config_diff_format_unified_is_default() {
    let left = temp_file_path("left-fmt-default");
    let right = temp_file_path("right-fmt-default");
    fs::write(&left, "hostname old\n").expect("write left");
    fs::write(&right, "hostname new\n").expect("write right");

    let explicit = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--format")
        .arg("unified")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --format unified");

    let implicit = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff (default format)");

    assert_eq!(
        explicit.stdout, implicit.stdout,
        "--format unified should produce the same output as the default"
    );
}

#[test]
fn config_diff_reads_file_a_from_stdin() {
    let right = temp_file_path("right-stdin-a");
    fs::write(&right, "hostname new\n").expect("write right");

    let mut child = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--no-color")
        .arg("-")
        .arg(&right)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn config-diff");

    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"hostname old\n")
        .expect("write stdin");

    let output = child.wait_with_output().expect("wait for config-diff");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("<stdin>"),
        "header should show <stdin> for the piped input"
    );
    assert!(
        stdout.contains("- hostname old"),
        "should contain deleted line"
    );
    assert!(
        stdout.contains("+ hostname new"),
        "should contain inserted line"
    );
}

#[test]
fn config_diff_reads_file_b_from_stdin() {
    let left = temp_file_path("left-stdin-b");
    fs::write(&left, "hostname old\n").expect("write left");

    let mut child = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--no-color")
        .arg(&left)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn config-diff");

    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"hostname new\n")
        .expect("write stdin");

    let output = child.wait_with_output().expect("wait for config-diff");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("<stdin>"),
        "header should show <stdin> for the piped input"
    );
    assert!(
        stdout.contains("- hostname old"),
        "should contain deleted line"
    );
    assert!(
        stdout.contains("+ hostname new"),
        "should contain inserted line"
    );
}

#[test]
fn config_diff_stdin_with_json_output() {
    let right = temp_file_path("right-stdin-json");
    fs::write(&right, "hostname new\n").expect("write right");

    let mut child = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--json")
        .arg("-")
        .arg(&right)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn config-diff");

    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"hostname old\n")
        .expect("write stdin");

    let output = child.wait_with_output().expect("wait for config-diff");
    assert!(output.status.success());
    let diff_json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid json from stdin input");
    assert_eq!(diff_json["has_changes"], true);
}

#[test]
fn config_diff_both_stdin_yields_no_changes() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--json")
        .arg("-")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn config-diff");

    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"hostname router\n")
        .expect("write stdin");

    let output = child.wait_with_output().expect("wait for config-diff");
    assert!(output.status.success(), "identical stdin → exit 0");
    let diff_json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid json");
    assert_eq!(
        diff_json["has_changes"], false,
        "both args are `-` → same content, no changes"
    );
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
