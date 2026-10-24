//! integration coverage for Junos identity keys.
//!
//! the value-change cases pin identities that are deliberately kept coarse.

use netform_dialects::junos::parse;
use netform_diff::{
    Diff, Edit, NormalizeOptions, OrderPolicy, OrderPolicyConfig, PlanAction, PlanLineEdit,
    PlanLineEditKind, build_plan, diff_documents, finding_code,
};

fn keyed_stable() -> NormalizeOptions {
    NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
        default: OrderPolicy::KeyedStable,
        overrides: Vec::new(),
    })
}

fn diff(before: &str, after: &str) -> Diff {
    diff_documents(&parse(before), &parse(after), keyed_stable())
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
fn a_changed_value_reads_as_one_replace_of_that_statement() {
    // (why, before, after, old line, new line, every finding forbidden)
    let cases: &[(&str, &str, &str, &str, &str, bool)] = &[
        (
            "only the time-zone value changed; domain-name merely moved",
            "\
set system domain-name example.com
set system time-zone UTC
set routing-options static route 0.0.0.0/0 next-hop 10.0.0.1
set routing-options autonomous-system 65001
",
            "\
set system time-zone Europe/Berlin
set system domain-name example.com
set routing-options static route 0.0.0.0/0 next-hop 10.0.0.1
set routing-options autonomous-system 65001
",
            "set system time-zone UTC",
            "set system time-zone Europe/Berlin",
            false,
        ),
        (
            "every other statement only moved",
            "\
set system host-name edge-1
set system domain-name example.com
set system time-zone UTC
set snmp community public authorization read-only
set snmp community private authorization read-write
set chassis alarm management-ethernet link-down ignore
",
            "\
set snmp community private authorization read-only
set chassis alarm management-ethernet link-down ignore
set system time-zone UTC
set system host-name edge-1
set snmp community public authorization read-only
set system domain-name example.com
",
            "set snmp community private authorization read-write",
            "set snmp community private authorization read-only",
            true,
        ),
        (
            "the prefix identifies the route, so the next hop is its value",
            "\
set routing-options static route 10.0.0.0/8 next-hop 192.0.2.1
set routing-options static route 172.16.0.0/12 next-hop 192.0.2.2
",
            "\
set routing-options static route 172.16.0.0/12 next-hop 192.0.2.2
set routing-options static route 10.0.0.0/8 next-hop 192.0.2.9
",
            "set routing-options static route 10.0.0.0/8 next-hop 192.0.2.1",
            "set routing-options static route 10.0.0.0/8 next-hop 192.0.2.9",
            false,
        ),
        (
            "discard and reject are the same slot on the route",
            "\
set routing-options static route 10.0.0.0/8 discard
set routing-options static route 172.16.0.0/12 next-hop 192.0.2.2
",
            "\
set routing-options static route 172.16.0.0/12 next-hop 192.0.2.2
set routing-options static route 10.0.0.0/8 reject
",
            "set routing-options static route 10.0.0.0/8 discard",
            "set routing-options static route 10.0.0.0/8 reject",
            false,
        ),
        (
            "a user has one password, so rotating it is a value change",
            "\
set system login user admin authentication encrypted-password \"$6$abc\"
set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@a\"
",
            "\
set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@a\"
set system login user admin authentication encrypted-password \"$6$xyz\"
",
            "set system login user admin authentication encrypted-password \"$6$abc\"",
            "set system login user admin authentication encrypted-password \"$6$xyz\"",
            false,
        ),
        (
            "only the description value changed; mtu merely moved",
            "\
set interfaces ge-0/0/0 description uplink
set interfaces ge-0/0/0 mtu 9192
set interfaces ge-0/0/0 unit 0 family inet address 10.0.0.1/24
",
            "\
set interfaces ge-0/0/0 mtu 9192
set interfaces ge-0/0/0 description uplink-2
set interfaces ge-0/0/0 unit 0 family inet address 10.0.0.1/24
",
            "set interfaces ge-0/0/0 description uplink",
            "set interfaces ge-0/0/0 description uplink-2",
            false,
        ),
        (
            "root has one password, so rotating it is a value change",
            "\
set system root-authentication encrypted-password \"$6$abc\"
set system root-authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@a\"
",
            "\
set system root-authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@a\"
set system root-authentication encrypted-password \"$6$xyz\"
",
            "set system root-authentication encrypted-password \"$6$abc\"",
            "set system root-authentication encrypted-password \"$6$xyz\"",
            false,
        ),
        (
            "deactivating an interface changes only its header",
            "\
interfaces {
    ge-0/0/0 {
        description uplink;
        unit 0 {
            family inet;
        }
    }
    ge-0/0/1 {
        description spare;
    }
}
",
            "\
interfaces {
    inactive: ge-0/0/0 {
        description uplink;
        unit 0 {
            family inet;
        }
    }
    ge-0/0/1 {
        description spare;
    }
}
",
            "    ge-0/0/0 {",
            "    inactive: ge-0/0/0 {",
            true,
        ),
        (
            "deactivating a stanza changes only its header",
            "\
interfaces {
    ge-0/0/0 {
        description uplink;
    }
}
",
            "\
inactive: interfaces {
    ge-0/0/0 {
        description uplink;
    }
}
",
            "interfaces {",
            "inactive: interfaces {",
            true,
        ),
        (
            "protecting and deactivating a leaf changes only that leaf",
            "\
system {
    host-name r1;
    services {
        ssh;
    }
}
",
            "\
system {
    protect: inactive: host-name r1;
    services {
        ssh;
    }
}
",
            "    host-name r1;",
            "    protect: inactive: host-name r1;",
            true,
        ),
    ];
    for &(why, before, after, old, new, no_findings) in cases {
        let diff = diff(before, after);

        assert_eq!(
            replaced_pairs(&diff),
            vec![(vec![old.to_string()], vec![new.to_string()])],
            "{why}"
        );
        if no_findings {
            assert!(diff.findings.is_empty(), "{why}: {:?}", diff.findings);
        } else {
            assert_no_ambiguous_keys(&diff);
        }
    }
}

#[test]
fn a_pure_reorder_reports_nothing() {
    for (case, before, after) in [
        (
            "reordered static route attributes report nothing",
            "\
set routing-options static route 10.0.0.0/8 next-hop 192.0.2.1
set routing-options static route 10.0.0.0/8 preference 5
set routing-options static route 10.0.0.0/8 metric 10
set routing-options static route 10.0.0.0/8 no-readvertise
",
            "\
set routing-options static route 10.0.0.0/8 no-readvertise
set routing-options static route 10.0.0.0/8 preference 5
set routing-options static route 10.0.0.0/8 next-hop 192.0.2.1
set routing-options static route 10.0.0.0/8 metric 10
",
        ),
        (
            "reordered user authentication entries report nothing",
            "\
set system login user admin class super-user
set system login user admin authentication encrypted-password \"$6$abc\"
set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@a\"
set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@b\"
",
            "\
set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@b\"
set system login user admin authentication encrypted-password \"$6$abc\"
set system login user admin class super-user
set system login user admin authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@a\"
",
        ),
        (
            "reordered qualified next hop attributes report nothing",
            "\
set routing-options static route 10.0.0.0/8 qualified-next-hop 192.0.2.1 preference 10
set routing-options static route 10.0.0.0/8 qualified-next-hop 192.0.2.1 metric 5
set routing-options static route 10.0.0.0/8 preference 5
",
            "\
set routing-options static route 10.0.0.0/8 preference 5
set routing-options static route 10.0.0.0/8 qualified-next-hop 192.0.2.1 metric 5
set routing-options static route 10.0.0.0/8 qualified-next-hop 192.0.2.1 preference 10
",
        ),
        (
            "reordered rib scoped qualified next hop attributes report nothing",
            "\
set routing-options rib inet6.0 static route ::/0 qualified-next-hop 2001:db8::1 preference 10
set routing-options rib inet6.0 static route ::/0 qualified-next-hop 2001:db8::1 metric 5
",
            "\
set routing-options rib inet6.0 static route ::/0 qualified-next-hop 2001:db8::1 metric 5
set routing-options rib inet6.0 static route ::/0 qualified-next-hop 2001:db8::1 preference 10
",
        ),
        (
            "reordered root authentication entries report nothing",
            "\
set system root-authentication encrypted-password \"$6$abc\"
set system root-authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@a\"
set system root-authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@b\"
set system root-authentication ssh-ed25519 \"AAAAC3NzaC1lZDI\"
",
            "\
set system root-authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@b\"
set system root-authentication ssh-ed25519 \"AAAAC3NzaC1lZDI\"
set system root-authentication encrypted-password \"$6$abc\"
set system root-authentication ssh-rsa \"AAAAB3NzaC1yc2E ops@a\"
",
        ),
        (
            "reordered ssh algorithm lists report nothing",
            "\
set system services ssh root-login deny
set system services ssh ciphers aes256-ctr
set system services ssh ciphers aes128-ctr
set system services ssh macs hmac-sha2-256
",
            "\
set system services ssh macs hmac-sha2-256
set system services ssh ciphers aes128-ctr
set system services ssh root-login deny
set system services ssh ciphers aes256-ctr
",
        ),
        (
            "set membership leaves stay distinct",
            "\
set system name-server 8.8.8.8
set system name-server 1.1.1.1
set security zones security-zone TRUST host-inbound-traffic system-services ping
set security zones security-zone TRUST host-inbound-traffic system-services ssh
",
            "\
set security zones security-zone TRUST host-inbound-traffic system-services ssh
set system name-server 1.1.1.1
set security zones security-zone TRUST host-inbound-traffic system-services ping
set system name-server 8.8.8.8
",
        ),
    ] {
        let diff = diff(before, after);

        assert!(
            !diff.has_changes,
            "{case}: pure reordering: {:?}",
            diff.edits
        );
        assert!(diff.findings.is_empty(), "{case}: {:?}", diff.findings);
    }
}

#[test]
fn a_removed_statement_reads_as_one_delete_of_that_statement() {
    for (case, before, removed) in [
        (
            "bgp neighbor",
            "\
set protocols bgp group EXT type external
set protocols bgp group EXT neighbor 192.0.2.1 peer-as 65100
set protocols bgp group EXT neighbor 192.0.2.2 peer-as 65200
",
            "set protocols bgp group EXT neighbor 192.0.2.1 peer-as 65100",
        ),
        (
            "prefix-list member",
            "\
set policy-options prefix-list INTERNAL 10.0.0.0/8
set policy-options prefix-list INTERNAL 172.16.0.0/12
set policy-options prefix-list INTERNAL 192.168.0.0/16
",
            "set policy-options prefix-list INTERNAL 10.0.0.0/8",
        ),
        (
            "interface address",
            "\
set interfaces ge-0/0/0 unit 0 family inet address 10.0.0.1/24
set interfaces ge-0/0/0 unit 0 family inet address 10.0.1.1/24
set interfaces ge-0/0/0 description uplink
",
            "set interfaces ge-0/0/0 unit 0 family inet address 10.0.0.1/24",
        ),
    ] {
        let after = before.replace(&format!("{removed}\n"), "");
        let diff = diff_documents(&parse(before), &parse(&after), NormalizeOptions::default())
            .expect("junos configs should diff");

        let deleted: Vec<Vec<&str>> = diff
            .edits
            .iter()
            .map(|edit| match edit {
                Edit::Delete { lines, .. } => lines.iter().map(|l| l.text.as_str()).collect(),
                other => panic!("{case}: expected only a delete, got {other:?}"),
            })
            .collect();
        assert_eq!(deleted, vec![vec![removed]], "{case}");
        assert_no_ambiguous_keys(&diff);

        let line_edits: Vec<PlanLineEdit> = build_plan(&diff)
            .actions
            .into_iter()
            .flat_map(|action| match action {
                PlanAction::ApplyLineEditsUnderContext { line_edits, .. } => line_edits,
                other => panic!("{case}: expected line edits, got {other:?}"),
            })
            .collect();
        assert_eq!(
            line_edits,
            vec![PlanLineEdit {
                kind: PlanLineEditKind::Delete,
                text: removed.to_string(),
            }],
            "{case}"
        );
    }
}
