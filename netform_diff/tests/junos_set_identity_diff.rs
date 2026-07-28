//! Integration coverage for Junos `set`-style identity keys under keyed-stable.
//!
//! Every test here fails on the pre-fix code, where a section's statements
//! shared one identity and were therefore paired with each other.

use netform_dialect_junos::parse_junos;
use netform_diff::{
    Diff, Edit, NormalizeOptions, OrderPolicy, OrderPolicyConfig, diff_documents, finding_code,
};

fn keyed_stable() -> NormalizeOptions {
    NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
        default: OrderPolicy::KeyedStable,
        overrides: Vec::new(),
    })
}

fn diff(before: &str, after: &str) -> Diff {
    diff_documents(&parse_junos(before), &parse_junos(after), keyed_stable())
        .expect("junos configs should diff")
}

fn replaced_pairs(diff: &Diff) -> Vec<(Vec<String>, Vec<String>)> {
    diff.edits
        .iter()
        .filter_map(|edit| match edit {
            Edit::Replace {
                old_lines,
                new_lines,
                ..
            } => Some((
                old_lines.iter().map(|l| l.text.clone()).collect(),
                new_lines.iter().map(|l| l.text.clone()).collect(),
            )),
            _ => None,
        })
        .collect()
}

fn assert_no_ambiguous_keys(diff: &Diff) {
    assert!(
        diff.findings
            .iter()
            .all(|f| f.code != finding_code::AMBIGUOUS_KEY_MATCH),
        "distinct statements must not share a content key: {:?}",
        diff.findings
    );
}

#[test]
fn reordered_system_statements_report_only_the_changed_value() {
    let before = "\
set system domain-name example.com
set system time-zone UTC
set routing-options static route 0.0.0.0/0 next-hop 10.0.0.1
set routing-options autonomous-system 65001
";
    let after = "\
set system time-zone Europe/Berlin
set system domain-name example.com
set routing-options static route 0.0.0.0/0 next-hop 10.0.0.1
set routing-options autonomous-system 65001
";

    let diff = diff(before, after);

    assert_eq!(
        replaced_pairs(&diff),
        vec![(
            vec!["set system time-zone UTC".to_string()],
            vec!["set system time-zone Europe/Berlin".to_string()],
        )],
        "only the time-zone value changed; domain-name merely moved"
    );
    assert_no_ambiguous_keys(&diff);
}

#[test]
fn reordering_across_sections_isolates_the_one_changed_statement() {
    let before = "\
set system host-name edge-1
set system domain-name example.com
set system time-zone UTC
set snmp community public authorization read-only
set snmp community private authorization read-write
set chassis alarm management-ethernet link-down ignore
";
    let after = "\
set snmp community private authorization read-only
set chassis alarm management-ethernet link-down ignore
set system time-zone UTC
set system host-name edge-1
set snmp community public authorization read-only
set system domain-name example.com
";

    let diff = diff(before, after);

    assert_eq!(
        replaced_pairs(&diff),
        vec![(
            vec!["set snmp community private authorization read-write".to_string()],
            vec!["set snmp community private authorization read-only".to_string()],
        )],
        "every other statement only moved"
    );
    assert!(diff.findings.is_empty(), "{:?}", diff.findings);
}

#[test]
fn static_route_next_hop_change_reads_as_a_value_change() {
    let before = "\
set routing-options static route 10.0.0.0/8 next-hop 192.0.2.1
set routing-options static route 172.16.0.0/12 next-hop 192.0.2.2
";
    let after = "\
set routing-options static route 172.16.0.0/12 next-hop 192.0.2.2
set routing-options static route 10.0.0.0/8 next-hop 192.0.2.9
";

    let diff = diff(before, after);

    assert_eq!(
        replaced_pairs(&diff),
        vec![(
            vec!["set routing-options static route 10.0.0.0/8 next-hop 192.0.2.1".to_string()],
            vec!["set routing-options static route 10.0.0.0/8 next-hop 192.0.2.9".to_string()],
        )],
        "the prefix identifies the route, so the next hop is its value"
    );
}

#[test]
fn reordered_static_route_attributes_report_nothing() {
    let before = "\
set routing-options static route 10.0.0.0/8 next-hop 192.0.2.1
set routing-options static route 10.0.0.0/8 preference 5
set routing-options static route 10.0.0.0/8 metric 10
set routing-options static route 10.0.0.0/8 no-readvertise
";
    let after = "\
set routing-options static route 10.0.0.0/8 no-readvertise
set routing-options static route 10.0.0.0/8 preference 5
set routing-options static route 10.0.0.0/8 next-hop 192.0.2.1
set routing-options static route 10.0.0.0/8 metric 10
";

    let diff = diff(before, after);

    assert!(!diff.has_changes, "pure reordering: {:?}", diff.edits);
    assert!(diff.findings.is_empty(), "{:?}", diff.findings);
}

#[test]
fn static_route_next_hop_type_change_reads_as_a_value_change() {
    let before = "\
set routing-options static route 10.0.0.0/8 discard
set routing-options static route 172.16.0.0/12 next-hop 192.0.2.2
";
    let after = "\
set routing-options static route 172.16.0.0/12 next-hop 192.0.2.2
set routing-options static route 10.0.0.0/8 reject
";

    let diff = diff(before, after);

    assert_eq!(
        replaced_pairs(&diff),
        vec![(
            vec!["set routing-options static route 10.0.0.0/8 discard".to_string()],
            vec!["set routing-options static route 10.0.0.0/8 reject".to_string()],
        )],
        "discard and reject are the same slot on the route"
    );
    assert_no_ambiguous_keys(&diff);
}

#[test]
fn reordered_user_authentication_entries_report_nothing() {
    let before = "\
set system login user admin class super-user
set system login user admin authentication encrypted-password \"$6$abc\"
set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@a\"
set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@b\"
";
    let after = "\
set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@b\"
set system login user admin authentication encrypted-password \"$6$abc\"
set system login user admin class super-user
set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@a\"
";

    let diff = diff(before, after);

    assert!(!diff.has_changes, "pure reordering: {:?}", diff.edits);
    assert!(diff.findings.is_empty(), "{:?}", diff.findings);
}

#[test]
fn user_password_change_reads_as_a_value_change() {
    let before = "\
set system login user admin authentication encrypted-password \"$6$abc\"
set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@a\"
";
    let after = "\
set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@a\"
set system login user admin authentication encrypted-password \"$6$xyz\"
";

    let diff = diff(before, after);

    assert_eq!(
        replaced_pairs(&diff),
        vec![(
            vec![
                "set system login user admin authentication encrypted-password \"$6$abc\""
                    .to_string()
            ],
            vec![
                "set system login user admin authentication encrypted-password \"$6$xyz\""
                    .to_string()
            ],
        )],
        "a user has one password, so rotating it is a value change"
    );
    assert_no_ambiguous_keys(&diff);
}

#[test]
fn reordered_ssh_algorithm_lists_report_nothing() {
    let before = "\
set system services ssh root-login deny
set system services ssh ciphers aes256-ctr
set system services ssh ciphers aes128-ctr
set system services ssh macs hmac-sha2-256
";
    let after = "\
set system services ssh macs hmac-sha2-256
set system services ssh ciphers aes128-ctr
set system services ssh root-login deny
set system services ssh ciphers aes256-ctr
";

    let diff = diff(before, after);

    assert!(!diff.has_changes, "pure reordering: {:?}", diff.edits);
    assert!(diff.findings.is_empty(), "{:?}", diff.findings);
}

#[test]
fn set_membership_leaves_stay_distinct() {
    let before = "\
set system name-server 8.8.8.8
set system name-server 1.1.1.1
set security zones security-zone TRUST host-inbound-traffic system-services ping
set security zones security-zone TRUST host-inbound-traffic system-services ssh
";
    let after = "\
set security zones security-zone TRUST host-inbound-traffic system-services ssh
set system name-server 1.1.1.1
set security zones security-zone TRUST host-inbound-traffic system-services ping
set system name-server 8.8.8.8
";

    let diff = diff(before, after);

    assert!(!diff.has_changes, "pure reordering: {:?}", diff.edits);
    assert!(diff.findings.is_empty(), "{:?}", diff.findings);
}
