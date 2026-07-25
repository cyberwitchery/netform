use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else {
            out.push(c);
        }
    }
    out
}

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
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
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
fn config_diff_exits_one_for_keyed_leaf_value_change() {
    let left = temp_file_path("keyed-leaf-left");
    let right = temp_file_path("keyed-leaf-right");
    fs::write(
        &left,
        "config system global\n    set hostname \"edge-1\"\nend\n",
    )
    .expect("write left");
    fs::write(
        &right,
        "config system global\n    set hostname \"edge-2\"\nend\n",
    )
    .expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--dialect")
        .arg("fortios")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff");

    assert_eq!(
        output.status.code(),
        Some(1),
        "a changed `set` value is drift: {}",
        strip_ansi(&String::from_utf8_lossy(&output.stdout))
    );

    let json_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--dialect")
        .arg("fortios")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --json");

    let diff_json: serde_json::Value =
        serde_json::from_slice(&json_output.stdout).expect("parse diff json");
    assert_eq!(diff_json["has_changes"], true);
    assert!(
        !diff_json["edits"]
            .as_array()
            .expect("edits array")
            .is_empty(),
        "edits must not be empty when has_changes is true"
    );
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
fn config_diff_root_level_reorder_is_silent_and_exits_zero() {
    let left = temp_file_path("left-root-reorder");
    let right = temp_file_path("right-root-reorder");
    fs::write(
        &left,
        "set system host-name edge-1\nset system domain-name example.com\n",
    )
    .expect("write left");
    fs::write(
        &right,
        "set system domain-name example.com\nset system host-name edge-1\n",
    )
    .expect("write right");

    for policy in ["unordered", "keyed-stable"] {
        let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
            .arg("--dialect")
            .arg("junos")
            .arg("--order-policy")
            .arg(policy)
            .arg(&left)
            .arg(&right)
            .output()
            .expect("run config-diff on a root-level reorder");

        assert!(
            output.status.success(),
            "{policy} should exit 0 on a pure root-level reorder"
        );
        let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        assert!(
            stdout.trim().is_empty(),
            "{policy} should report nothing on a pure root-level reorder: {stdout}"
        );

        let json_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
            .arg("--no-exit-code")
            .arg("--dialect")
            .arg("junos")
            .arg("--order-policy")
            .arg(policy)
            .arg("--json")
            .arg(&left)
            .arg(&right)
            .output()
            .expect("run config-diff --json on a root-level reorder");
        let diff_json: serde_json::Value =
            serde_json::from_slice(&json_output.stdout).expect("valid json");
        assert_eq!(diff_json["has_changes"], false, "{policy}");
        assert_eq!(
            diff_json["findings"].as_array().map(Vec::len),
            Some(0),
            "{policy} should not warn about an unreliable region when nothing changed: {}",
            diff_json["findings"]
        );
    }
}

#[test]
fn config_diff_root_level_reorder_with_a_change_still_exits_nonzero() {
    let left = temp_file_path("left-root-reorder-change");
    let right = temp_file_path("right-root-reorder-change");
    fs::write(
        &left,
        "set system host-name edge-1\nset system ntp server 10.0.0.1\n",
    )
    .expect("write left");
    fs::write(
        &right,
        "set system ntp server 10.0.0.1\nset system host-name edge-2\n",
    )
    .expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--dialect")
        .arg("junos")
        .arg("--order-policy")
        .arg("unordered")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff on a root-level reorder with a change");

    assert_eq!(output.status.code(), Some(1));
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("- set system host-name edge-1")
            && stdout.contains("+ set system host-name edge-2"),
        "the value change must still be reported: {stdout}"
    );
    assert!(
        !stdout.contains("ntp server"),
        "the reordered-but-unchanged line must not be reported: {stdout}"
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
    // use structural change (add a line) to produce a diff — FortiOS leaf-line
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
    // structural change (remove a line) to produce unified diff output.
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
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
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
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
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
fn config_diff_auto_dialect_detects_junos() {
    let left = temp_file_path("left-auto-junos");
    let right = temp_file_path("right-auto-junos");
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

    // without --dialect (defaults to auto), should behave like --dialect junos.
    let auto_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff (auto)");

    let explicit_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--dialect")
        .arg("junos")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --dialect junos");

    assert!(auto_output.status.success());
    assert_eq!(
        auto_output.stdout, explicit_output.stdout,
        "auto-detected dialect should produce same output as explicit --dialect junos"
    );
}

#[test]
fn config_diff_auto_dialect_detects_fortios() {
    let left = temp_file_path("left-auto-fortios");
    let right = temp_file_path("right-auto-fortios");
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

    let auto_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff (auto)");

    let explicit_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--dialect")
        .arg("fortios")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --dialect fortios");

    assert!(auto_output.status.success());
    assert_eq!(
        auto_output.stdout, explicit_output.stdout,
        "auto-detected dialect should produce same output as explicit --dialect fortios"
    );
}

#[test]
fn config_diff_auto_dialect_detects_nxos() {
    let left = temp_file_path("left-auto-nxos");
    let right = temp_file_path("right-auto-nxos");
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

    let auto_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff (auto)");

    let explicit_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--dialect")
        .arg("nxos")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --dialect nxos");

    assert!(auto_output.status.success());
    assert_eq!(
        auto_output.stdout, explicit_output.stdout,
        "auto-detected dialect should produce same output as explicit --dialect nxos"
    );
}

#[test]
fn config_diff_auto_dialect_detects_eos() {
    let left = temp_file_path("left-auto-eos");
    let right = temp_file_path("right-auto-eos");
    fs::write(
        &left,
        "hostname leaf-01\ninterface Ethernet1\n   description old-uplink\n   mtu 9214\n   ip address 192.0.2.2/31\n   no shutdown\nip access-list ACL-EDGE-IN\n   10 permit tcp 10.10.1.0/24 any eq https\n   20 permit tcp 10.10.1.0/24 any eq ssh\n   90 deny ip any any log\n",
    )
    .expect("write left");
    fs::write(
        &right,
        "hostname leaf-01\ninterface Ethernet1\n   description new-uplink\n   mtu 9214\n   ip address 192.0.2.2/31\n   no shutdown\nip access-list ACL-EDGE-IN\n   10 permit tcp 10.10.1.0/24 any eq https\n   20 permit tcp 10.10.1.0/24 any eq ssh\n   90 deny ip any any log\n",
    )
    .expect("write right");

    let auto_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff (auto)");

    let explicit_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--dialect")
        .arg("eos")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --dialect eos");

    assert!(auto_output.status.success());
    assert_eq!(
        auto_output.stdout, explicit_output.stdout,
        "auto-detected dialect should produce same output as explicit --dialect eos"
    );
}

#[test]
fn config_diff_auto_dialect_detects_iosxe() {
    let left = temp_file_path("left-auto-iosxe");
    let right = temp_file_path("right-auto-iosxe");
    fs::write(
        &left,
        "hostname rtr-01\ninterface GigabitEthernet0/0/0\n  description old-uplink\n  ip address 192.0.2.2 255.255.255.252\n  no shutdown\nrouter bgp 65000\n  address-family ipv4 unicast\n    network 10.10.1.0 mask 255.255.255.0\nip access-list extended ACL-EDGE-IN\n  permit tcp 10.10.1.0 0.0.0.255 any eq 443\n  deny ip any any log\n",
    )
    .expect("write left");
    fs::write(
        &right,
        "hostname rtr-01\ninterface GigabitEthernet0/0/0\n  description new-uplink\n  ip address 192.0.2.2 255.255.255.252\n  no shutdown\nrouter bgp 65000\n  address-family ipv4 unicast\n    network 10.10.1.0 mask 255.255.255.0\nip access-list extended ACL-EDGE-IN\n  permit tcp 10.10.1.0 0.0.0.255 any eq 443\n  deny ip any any log\n",
    )
    .expect("write right");

    let auto_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff (auto)");

    let explicit_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--dialect")
        .arg("iosxe")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --dialect iosxe");

    assert!(auto_output.status.success());
    assert_eq!(
        auto_output.stdout, explicit_output.stdout,
        "auto-detected dialect should produce same output as explicit --dialect iosxe"
    );
}

#[test]
fn config_diff_explicit_dialect_overrides_auto() {
    // feed Junos content but force --dialect generic — the explicit flag wins.
    let left = temp_file_path("left-override");
    let right = temp_file_path("right-override");
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

    let generic_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--dialect")
        .arg("generic")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff --dialect generic");

    let auto_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff (auto)");

    assert!(generic_output.status.success());
    assert!(auto_output.status.success());
    // Generic and Junos parse differently, so outputs should differ.
    assert_ne!(
        generic_output.stdout, auto_output.stdout,
        "explicit --dialect generic should differ from auto-detected junos"
    );
}

#[test]
fn config_diff_auto_dialect_accepts_value() {
    // verify the CLI accepts `--dialect auto` explicitly.
    let path = temp_file_path("auto-value");
    fs::write(&path, "hostname router\n").expect("write file");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--dialect")
        .arg("auto")
        .arg(&path)
        .arg(&path)
        .output()
        .expect("run config-diff --dialect auto");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--dialect auto should be accepted and produce exit 0 for identical files"
    );
}

#[test]
fn config_diff_policy_override_makes_subtree_unordered() {
    let left = temp_file_path("left-override-sub");
    let right = temp_file_path("right-override-sub");
    // two top-level blocks: "router bgp" (index 0) has reordered children.
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

    // without override: ordered default sees changes.
    let ordered_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--json")
        .arg("--no-exit-code")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff ordered");

    assert!(ordered_output.status.success());
    let ordered_json: serde_json::Value =
        serde_json::from_slice(&ordered_output.stdout).expect("valid json");
    assert_eq!(
        ordered_json["has_changes"], true,
        "ordered default should see reordering as a change"
    );

    // with --policy-override 0:unordered, the subtree ignores order.
    let override_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--json")
        .arg("--policy-override")
        .arg("0:unordered")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff with --policy-override");

    assert!(override_output.status.success());
    let override_json: serde_json::Value =
        serde_json::from_slice(&override_output.stdout).expect("valid json");
    assert_eq!(
        override_json["has_changes"], false,
        "--policy-override 0:unordered should ignore reordering in that subtree"
    );
}

#[test]
fn config_diff_multiple_policy_overrides() {
    let left = temp_file_path("left-multi-override");
    let right = temp_file_path("right-multi-override");
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

    // multiple --policy-override flags are accepted.
    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--json")
        .arg("--policy-override")
        .arg("0:unordered")
        .arg("--policy-override")
        .arg("0.1:keyed-stable")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff with multiple --policy-override");

    assert!(output.status.success());
    let diff_json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid json");
    // the overrides should be present in the output JSON.
    let overrides = &diff_json["order_policy"]["overrides"];
    assert_eq!(overrides.as_array().map(|a| a.len()), Some(2));
}

#[test]
fn config_diff_policy_override_appears_in_json_output() {
    let path = temp_file_path("override-json");
    fs::write(&path, "hostname router\n").expect("write file");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--json")
        .arg("--policy-override")
        .arg("0:keyed-stable")
        .arg(&path)
        .arg(&path)
        .output()
        .expect("run config-diff with --policy-override");

    assert!(output.status.success());
    let diff_json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let overrides = &diff_json["order_policy"]["overrides"];
    assert!(overrides.is_array());
    assert_eq!(overrides[0]["context_prefix"], serde_json::json!([0]));
    assert_eq!(overrides[0]["policy"], "keyed-stable");
}

#[test]
fn config_diff_policy_override_invalid_format_fails() {
    let path = temp_file_path("override-bad");
    fs::write(&path, "hostname router\n").expect("write file");

    // missing colon separator.
    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--policy-override")
        .arg("0-unordered")
        .arg(&path)
        .arg(&path)
        .output()
        .expect("run config-diff with bad --policy-override");

    assert!(
        !output.status.success(),
        "invalid override format should fail"
    );
}

#[test]
fn config_diff_policy_override_invalid_policy_fails() {
    let path = temp_file_path("override-bad-policy");
    fs::write(&path, "hostname router\n").expect("write file");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--policy-override")
        .arg("0:bogus")
        .arg(&path)
        .arg(&path)
        .output()
        .expect("run config-diff with bad policy name");

    assert!(!output.status.success(), "unknown policy should fail");
}

#[test]
fn config_diff_auto_dialect_disagreement_falls_back_to_generic() {
    // file A is strongly Junos; file B is strongly FortiOS.
    // auto-detection disagrees → should fall back to Generic.
    let junos_file = temp_file_path("left-disagree-junos");
    let fortios_file = temp_file_path("right-disagree-fortios");
    fs::write(
        &junos_file,
        "interfaces {\n    ge-0/0/0 {\n        description \"uplink\";\n        mtu 9216;\n    }\n}\n",
    )
    .expect("write junos file");
    fs::write(
        &fortios_file,
        "config system global\n    set hostname \"FGT\"\n    set timezone 04\nend\n",
    )
    .expect("write fortios file");

    let auto_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--json")
        .arg(&junos_file)
        .arg(&fortios_file)
        .output()
        .expect("run config-diff (auto, disagreement)");

    let generic_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--dialect")
        .arg("generic")
        .arg("--json")
        .arg(&junos_file)
        .arg(&fortios_file)
        .output()
        .expect("run config-diff --dialect generic");

    assert!(auto_output.status.success());
    assert_eq!(
        auto_output.stdout, generic_output.stdout,
        "auto-detection disagreement should produce same output as explicit --dialect generic"
    );

    let stderr = String::from_utf8_lossy(&auto_output.stderr);
    assert!(
        stderr.contains("warning") && stderr.contains("disagree"),
        "should warn on stderr when auto-detected dialects disagree: {stderr}"
    );
    assert!(
        stderr.contains("junos") && stderr.contains("fortios"),
        "warning should name both detected dialects: {stderr}"
    );
}

#[test]
fn config_diff_auto_dialect_disagreement_nxos_vs_eos() {
    let nxos_file = temp_file_path("left-disagree-nxos");
    let eos_file = temp_file_path("right-disagree-eos");
    fs::write(
        &nxos_file,
        "feature bgp\nfeature interface-vlan\ninterface Ethernet1/1\n  description uplink\n  no shutdown\n",
    )
    .expect("write nxos file");
    fs::write(
        &eos_file,
        "interface Ethernet1\n   description uplink\n   mtu 9214\n   ip address 192.0.2.2/31\n   no shutdown\nip access-list ACL-EDGE-IN\n   10 permit tcp 10.10.1.0/24 any eq https\n   90 deny ip any any log\n",
    )
    .expect("write eos file");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--json")
        .arg(&nxos_file)
        .arg(&eos_file)
        .output()
        .expect("run config-diff (auto, nxos vs eos)");

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning") && stderr.contains("disagree"),
        "should warn when nxos and eos disagree: {stderr}"
    );
    assert!(
        stderr.contains("nxos") && stderr.contains("eos"),
        "warning should name both detected dialects: {stderr}"
    );
}

#[test]
fn config_diff_auto_dialect_disagreement_named_vs_generic() {
    // one file detects as a named dialect, the other is too ambiguous.
    let junos_file = temp_file_path("left-disagree-named");
    let ambiguous_file = temp_file_path("right-disagree-ambiguous");
    fs::write(
        &junos_file,
        "interfaces {\n    ge-0/0/0 {\n        description \"uplink\";\n        mtu 9216;\n    }\n}\n",
    )
    .expect("write junos file");
    fs::write(&ambiguous_file, "hostname router\n").expect("write ambiguous file");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--json")
        .arg(&junos_file)
        .arg(&ambiguous_file)
        .output()
        .expect("run config-diff (auto, named vs generic)");

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning") && stderr.contains("disagree"),
        "should warn when named and generic disagree: {stderr}"
    );
    assert!(
        stderr.contains("junos") && stderr.contains("generic"),
        "warning should name both detected hints: {stderr}"
    );
}

#[test]
fn config_diff_auto_dialect_agreement_no_warning() {
    // both files are clearly Junos — no warning should appear.
    let left = temp_file_path("left-agree-junos");
    let right = temp_file_path("right-agree-junos");
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
        .arg("--json")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff (auto, agreement)");

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("disagree"),
        "no disagreement warning when both files detect as the same dialect: {stderr}"
    );
}

#[test]
fn config_diff_explicit_dialect_no_disagreement_warning() {
    // even with mismatched content, an explicit --dialect skips auto-detection.
    let junos_file = temp_file_path("left-explicit-no-warn");
    let fortios_file = temp_file_path("right-explicit-no-warn");
    fs::write(
        &junos_file,
        "interfaces {\n    ge-0/0/0 {\n        description \"uplink\";\n        mtu 9216;\n    }\n}\n",
    )
    .expect("write junos file");
    fs::write(
        &fortios_file,
        "config system global\n    set hostname \"FGT\"\n    set timezone 04\nend\n",
    )
    .expect("write fortios file");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg("--dialect")
        .arg("generic")
        .arg("--json")
        .arg(&junos_file)
        .arg(&fortios_file)
        .output()
        .expect("run config-diff (explicit dialect, no warning)");

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("disagree"),
        "no disagreement warning when dialect is explicitly set: {stderr}"
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
