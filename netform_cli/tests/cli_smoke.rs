use std::fs;
use std::path::{Path, PathBuf};
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
    let edits = diff_json["edits"].as_array().expect("edits array");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0]["type"], "Replace");

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
    assert_eq!(plan_json["version"], "v1");
    assert_eq!(
        plan_json["actions"]
            .as_array()
            .expect("actions array")
            .len(),
        1
    );
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
fn config_diff_reports_actions_swapped_between_fortios_policies_under_every_policy() {
    let left = temp_file_path("left-fortios-action-swap");
    let right = temp_file_path("right-fortios-action-swap");
    let policies = |first: &str, second: &str| {
        format!(
            "config firewall policy\n    edit 1\n        set action {first}\n    next\n    edit 2\n        set action {second}\n    next\nend\n"
        )
    };
    fs::write(&left, policies("accept", "deny")).expect("write left");
    fs::write(&right, policies("deny", "accept")).expect("write right");

    for policy in ["ordered", "unordered", "keyed-stable"] {
        let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
            .arg("--dialect")
            .arg("fortios")
            .arg("--order-policy")
            .arg(policy)
            .arg(&left)
            .arg(&right)
            .output()
            .expect("run config-diff on swapped fortios policy actions");
        assert_eq!(
            output.status.code(),
            Some(1),
            "{policy} must exit 1 when two policies swap their actions"
        );

        let json_output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
            .arg("--no-exit-code")
            .arg("--dialect")
            .arg("fortios")
            .arg("--order-policy")
            .arg(policy)
            .arg("--json")
            .arg(&left)
            .arg(&right)
            .output()
            .expect("run config-diff --json on swapped fortios policy actions");
        let diff_json: serde_json::Value =
            serde_json::from_slice(&json_output.stdout).expect("valid json");
        assert_eq!(diff_json["has_changes"], true, "{policy}");
    }
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
fn config_diff_policy_override_on_a_renamed_root_segment() {
    let left = temp_file_path("left-override-renamed-root");
    let right = temp_file_path("right-override-renamed-root");
    fs::write(
        &left,
        "ip access-list extended ACL-IN\n permit tcp any any eq 443\n permit tcp any any eq 80\ninterface Gi0/1\n description uplink\n",
    )
    .expect("write left");
    fs::write(
        &right,
        "ip access-list extended ACL-OUT\n permit tcp any any eq 80\n permit tcp any any eq 443\ninterface Gi0/1\n description uplink\n",
    )
    .expect("write right");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--dialect")
        .arg("iosxe")
        .arg("--policy-override")
        .arg("0:unordered")
        .arg(&left)
        .arg(&right)
        .output()
        .expect("run config-diff with an override on a renamed root segment");

    assert_eq!(output.status.code(), Some(1));
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("- ip access-list extended ACL-IN")
            && stdout.contains("+ ip access-list extended ACL-OUT"),
        "the rename must still be reported: {stdout}"
    );
    assert!(
        stdout.contains("@@ delete 3 line(s)")
            && stdout.contains("@@ insert 3 line(s)")
            && !stdout.contains("@@ replace"),
        "--policy-override 0:unordered must report each ACL whole, not the ordered replace: {stdout}"
    );
}

#[test]
fn config_diff_policy_override_invalid_format_fails() {
    let path = temp_file_path("override-bad");
    fs::write(&path, "hostname router\n").expect("write file");

    // missing colon separator, unknown policy.
    for (value, message) in [
        ("0-unordered", "expected PATH:POLICY"),
        ("0:bogus", "unknown policy \"bogus\""),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
            .arg("--policy-override")
            .arg(value)
            .arg(&path)
            .arg(&path)
            .output()
            .expect("run config-diff with bad --policy-override");

        assert_eq!(output.status.code(), Some(2), "{value}: usage error");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(message), "{value}: {stderr}");
    }
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
        stderr.is_empty(),
        "no warning when both files detect as the same dialect: {stderr}"
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
        stderr.is_empty(),
        "no warning when dialect is explicitly set: {stderr}"
    );
}

#[test]
fn replay_fixtures_cli_runs_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_netform-replay-fixtures"))
        .output()
        .expect("run replay binary");

    assert!(output.status.success());
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures");
    let count = fs::read_dir(&fixtures)
        .expect("read fixtures dir")
        .filter(|entry| {
            entry
                .as_ref()
                .expect("fixture entry")
                .path()
                .extension()
                .is_some_and(|ext| ext == "json")
        })
        .count();
    assert!(count > 0, "fixtures/ should hold json fixtures");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), format!("replayed {count} fixture(s)"));
}

fn color_input_pair() -> (PathBuf, PathBuf) {
    let left = temp_file_path("left-color");
    let right = temp_file_path("right-color");
    fs::write(&left, "hostname old\n").expect("write left");
    fs::write(&right, "hostname new\n").expect("write right");
    (left, right)
}

/// reads stdout raw: stripping ANSI here would discard the bytes under test.
fn color_stdout(left: &Path, right: &Path, args: &[&str], env: &[(&str, &str)]) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_config-diff"));
    cmd.arg("--no-exit-code").env_remove("NO_COLOR");
    for (key, value) in env {
        cmd.env(key, value);
    }
    for arg in args {
        cmd.arg(arg);
    }
    let output = cmd.arg(left).arg(right).output().expect("run config-diff");

    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn color_flag_stdout(args: &[&str], env: &[(&str, &str)]) -> String {
    let (left, right) = color_input_pair();
    color_stdout(&left, &right, args, env)
}

#[test]
fn config_diff_no_color_emits_no_ansi_escapes() {
    let stdout = color_flag_stdout(&["--no-color"], &[]);

    assert!(stdout.contains("--- "), "should still print the diff");
    assert!(
        !stdout.contains('\x1b'),
        "--no-color should suppress ANSI escapes: {stdout:?}"
    );
}

#[test]
fn config_diff_color_forces_ansi_escapes_off_a_tty() {
    let stdout = color_flag_stdout(&["--color"], &[]);

    assert!(
        stdout.contains("\x1b[1m--- "),
        "--color should emit ANSI escapes on a pipe: {stdout:?}"
    );
}

#[test]
fn config_diff_defaults_to_plain_output_off_a_tty() {
    let stdout = color_flag_stdout(&[], &[]);

    assert!(stdout.contains("--- "), "should still print the diff");
    assert!(
        !stdout.contains('\x1b'),
        "a piped stdout should get no ANSI escapes: {stdout:?}"
    );
}

#[test]
fn config_diff_color_off_matches_color_on_without_escapes() {
    let (left, right) = color_input_pair();
    let colored = color_stdout(&left, &right, &["--color"], &[]);
    let uncolored = color_stdout(&left, &right, &["--no-color"], &[]);

    assert!(colored.contains('\x1b'));
    assert_eq!(strip_ansi(&colored), uncolored);
}

#[test]
fn config_diff_color_flag_overrides_no_color_environment_variable() {
    let stdout = color_flag_stdout(&["--color"], &[("NO_COLOR", "1")]);

    assert!(
        stdout.contains("\x1b[1m--- "),
        "--color should win over NO_COLOR: {stdout:?}"
    );
}

#[test]
fn config_diff_markdown_format_is_never_colored() {
    let stdout = color_flag_stdout(&["--color", "--format", "markdown"], &[]);

    assert!(stdout.contains("# Config Diff Report"));
    assert!(
        !stdout.contains('\x1b'),
        "the markdown report carries no ANSI escapes: {stdout:?}"
    );
}

#[test]
fn config_diff_auto_dialect_detects_iosxr() {
    let xr = temp_file_path("iosxr-auto");
    let nxos = temp_file_path("nxos-auto");
    fs::write(
        &xr,
        "interface TenGigE0/0/0/0\n ipv4 address 192.0.2.1 255.255.255.252\nroute-policy PASS-ALL\n  pass\nend-policy\n",
    )
    .expect("write iosxr file");
    fs::write(
        &nxos,
        "feature bgp\nfeature interface-vlan\ninterface Ethernet1/1\n  ip address 192.0.2.2/31\n",
    )
    .expect("write nxos file");

    let output = Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .arg("--no-exit-code")
        .arg(&xr)
        .arg(&nxos)
        .output()
        .expect("run config-diff with auto dialect");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("iosxr vs nxos"),
        "auto detection should name iosxr, got: {stderr}",
    );
}

/// runs config-diff with `args` on two temp files holding `left` and `right`.
fn run_on(prefix: &str, args: &[&str], left: &str, right: &str) -> std::process::Output {
    let left_path = temp_file_path(&format!("left-{prefix}"));
    let right_path = temp_file_path(&format!("right-{prefix}"));
    fs::write(&left_path, left).expect("write left");
    fs::write(&right_path, right).expect("write right");
    Command::new(env!("CARGO_BIN_EXE_config-diff"))
        .args(args)
        .arg(&left_path)
        .arg(&right_path)
        .output()
        .expect("run config-diff")
}

#[test]
fn config_diff_cli_prints_unified_diff() {
    // (temp file prefix, args, left, right, lines stdout must contain)
    type Case<'a> = (&'a str, &'a [&'a str], &'a str, &'a str, &'a [&'a str]);
    let cases: &[Case] = &[
        (
            "unified",
            &["--no-exit-code"],
            "hostname old\n",
            "hostname new\n",
            &["---", "+++", "@@", "- hostname old", "+ hostname new"],
        ),
        // a structural change (one removed line) under fortios.
        (
            "fortios-unified",
            &["--no-exit-code", "--dialect", "fortios", "--no-color"],
            "config system global\n    set hostname \"FGT\"\n    set timezone 04\nend\n",
            "config system global\n    set hostname \"FGT\"\nend\n",
            &["---", "+++", "@@", "-     set timezone 04"],
        ),
    ];
    for (prefix, args, left, right, expected) in cases {
        let output = run_on(prefix, args, left, right);
        assert!(output.status.success(), "{prefix}");
        let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        for line in *expected {
            assert!(
                stdout.contains(line),
                "{prefix}: missing {line:?} in {stdout}"
            );
        }
    }
}

#[test]
fn config_diff_exits_zero_when_no_changes() {
    let nxos = "feature bgp\nvpc domain 10\n  peer-keepalive destination 10.0.0.1\n";
    let fortios =
        "config firewall address\n    edit \"all\"\n        set type ipmask\n    next\nend\n";
    let cases: &[(&[&str], &str)] = &[
        (&[], "hostname router\n"),
        // `auto` is the default, but the explicit value must be accepted too.
        (&["--dialect", "auto"], "hostname router\n"),
        (&["--dialect", "nxos"], nxos),
        (&["--dialect", "fortios"], fortios),
    ];
    for (args, content) in cases {
        let output = run_on("exit-code-same", args, content, content);
        assert_eq!(
            output.status.code(),
            Some(0),
            "identical files → exit 0: {args:?}"
        );
    }
}

#[test]
fn config_diff_keyed_stable_matches_reordered_children_per_dialect() {
    let customer = "route-policy CUSTOMER-IN\n  pass\nend-policy\n";
    let transit = "prefix-set TRANSIT-PFX\n  10.0.0.0/8 le 24\nend-set\n";
    let cases: [(&str, String, String); 3] = [
        (
            "junos",
            "interfaces {\n    ge-0/0/0 {\n        description \"uplink\";\n        mtu 9000;\n    }\n}\n".into(),
            "interfaces {\n    ge-0/0/0 {\n        mtu 9000;\n        description \"uplink\";\n    }\n}\n".into(),
        ),
        (
            "nxos",
            "interface Ethernet1/1\n  description uplink\n  mtu 9000\n".into(),
            "interface Ethernet1/1\n  mtu 9000\n  description uplink\n".into(),
        ),
        // reordered route-policy and prefix-set blocks match on their keys.
        (
            "iosxr",
            format!("{customer}{transit}"),
            format!("{transit}{customer}"),
        ),
    ];
    for (dialect, left, right) in &cases {
        let output = run_on(
            &format!("{dialect}-keyed"),
            &["--dialect", dialect, "--order-policy", "keyed-stable"],
            left,
            right,
        );
        let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        assert_eq!(output.status.code(), Some(0), "{dialect}: {stdout}");
        assert_eq!(stdout, "", "{dialect}");
    }
}

#[test]
fn config_diff_auto_dialect_matches_the_explicit_dialect() {
    let eos = |desc: &str| {
        format!(
            "hostname leaf-01\ninterface Ethernet1\n   description {desc}\n   mtu 9214\n   ip address 192.0.2.2/31\n   no shutdown\nip access-list ACL-EDGE-IN\n   10 permit tcp 10.10.1.0/24 any eq https\n   20 permit tcp 10.10.1.0/24 any eq ssh\n   90 deny ip any any log\n"
        )
    };
    let iosxe = |desc: &str| {
        format!(
            "hostname rtr-01\ninterface GigabitEthernet0/0/0\n  description {desc}\n  ip address 192.0.2.2 255.255.255.252\n  no shutdown\nrouter bgp 65000\n  address-family ipv4 unicast\n    network 10.10.1.0 mask 255.255.255.0\nip access-list extended ACL-EDGE-IN\n  permit tcp 10.10.1.0 0.0.0.255 any eq 443\n  deny ip any any log\n"
        )
    };
    let cases: [(&str, String, String); 7] = [
        (
            "junos",
            "interfaces {\n    ge-0/0/0 {\n        description \"a\";\n    }\n}\n".into(),
            "interfaces {\n    ge-0/0/0 {\n        description \"b\";\n    }\n}\n".into(),
        ),
        (
            "fortios",
            "config system global\n    set hostname \"FGT\"\nend\n".into(),
            "config system global\n    set hostname \"FGT\"\n    set timezone 04\nend\n".into(),
        ),
        (
            "nxos",
            "feature ospf\ninterface Ethernet1/1\n  description old-uplink\n  no shutdown\n".into(),
            "feature ospf\ninterface Ethernet1/1\n  description new-uplink\n  no shutdown\n".into(),
        ),
        ("eos", eos("old-uplink"), eos("new-uplink")),
        ("iosxe", iosxe("old-uplink"), iosxe("new-uplink")),
        (
            "iosxr",
            "route-policy CUSTOMER-IN\n  set community NO-EXPORT-SET\n  pass\nend-policy\n".into(),
            "route-policy CUSTOMER-IN\n  set community CUSTOMER-SET\n  pass\nend-policy\n".into(),
        ),
        (
            "iosxr",
            "interface Bundle-Ether10\n description old-uplink\n ipv4 address 192.0.2.1 255.255.255.252\n".into(),
            "interface Bundle-Ether10\n description new-uplink\n ipv4 address 192.0.2.1 255.255.255.252\n".into(),
        ),
    ];
    for (dialect, left, right) in &cases {
        let run = |extra: &[&str]| {
            let mut args = vec!["--no-exit-code", "--json"];
            args.extend_from_slice(extra);
            run_on(&format!("auto-{dialect}"), &args, left, right)
        };
        let auto = run(&[]);
        let explicit = run(&["--dialect", dialect]);
        let generic = run(&["--dialect", "generic"]);

        assert!(auto.status.success(), "{dialect}");
        assert_eq!(
            auto.stdout, explicit.stdout,
            "auto-detected dialect should produce same output as explicit --dialect {dialect}"
        );
        let diff_json: serde_json::Value =
            serde_json::from_slice(&explicit.stdout).expect("valid json");
        assert_eq!(diff_json["has_changes"], true, "{dialect}");
        // a vendor parser that fell back to generic would match the explicit run too.
        assert_ne!(
            explicit.stdout, generic.stdout,
            "--dialect {dialect} should not parse like generic"
        );
    }
}

#[test]
fn config_diff_policy_overrides_appear_in_json_output() {
    let output = run_on(
        "multi-override",
        &[
            "--json",
            "--policy-override",
            "0:unordered",
            "--policy-override",
            "0.1:keyed-stable",
        ],
        "router bgp 65000\n  neighbor 10.0.0.1\n  neighbor 10.0.0.2\n",
        "router bgp 65000\n  neighbor 10.0.0.2\n  neighbor 10.0.0.1\n",
    );

    assert!(output.status.success());
    let diff_json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid json");
    assert_eq!(
        diff_json["order_policy"]["overrides"],
        serde_json::json!([
            {"context_prefix": [0], "policy": "unordered"},
            {"context_prefix": [0, 1], "policy": "keyed-stable"},
        ])
    );
}

#[test]
fn config_diff_auto_dialect_disagreement_falls_back_to_generic() {
    let junos = "interfaces {\n    ge-0/0/0 {\n        description \"uplink\";\n        mtu 9216;\n    }\n}\n";
    let fortios = "config system global\n    set hostname \"FGT\"\n    set timezone 04\nend\n";
    let nxos = "feature bgp\nfeature interface-vlan\ninterface Ethernet1/1\n  description uplink\n  no shutdown\n";
    let eos = "interface Ethernet1\n   description uplink\n   mtu 9214\n   ip address 192.0.2.2/31\n   no shutdown\nip access-list ACL-EDGE-IN\n   10 permit tcp 10.10.1.0/24 any eq https\n   90 deny ip any any log\n";
    // (left, right, the two hints the warning names)
    let cases = [
        (junos, fortios, "junos", "fortios"),
        (nxos, eos, "nxos", "eos"),
        // one side is too ambiguous to detect.
        (junos, "hostname router\n", "junos", "generic"),
    ];
    for (left, right, a, b) in cases {
        let auto = run_on("disagree", &["--no-exit-code", "--json"], left, right);
        let generic = run_on(
            "disagree-generic",
            &["--no-exit-code", "--dialect", "generic", "--json"],
            left,
            right,
        );

        assert!(auto.status.success(), "{a} vs {b}");
        assert_eq!(
            auto.stdout, generic.stdout,
            "{a} vs {b}: disagreement should produce same output as explicit --dialect generic"
        );
        let stderr = String::from_utf8_lossy(&auto.stderr);
        assert!(
            stderr.contains("warning") && stderr.contains("disagree"),
            "{a} vs {b}: should warn on stderr: {stderr}"
        );
        assert!(
            stderr.contains(a) && stderr.contains(b),
            "warning should name both detected hints: {stderr}"
        );
    }
}
