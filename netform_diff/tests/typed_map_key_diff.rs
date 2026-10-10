//! typed NX-OS class-maps and policy-maps pair by name under keyed-stable.

use netform_dialects::nxos::parse;
use netform_diff::{
    Diff, Edit, NormalizeOptions, OrderPolicy, OrderPolicyConfig, diff_documents, finding_code,
};

const BEFORE: &str = "\
class-map type control-plane match-any copp-s-bgp
  match access-group name copp-system-acl-bgp
class-map type control-plane match-any copp-s-ospf
  match access-group name copp-system-acl-ospf
class-map type control-plane match-any copp-s-icmp
  match access-group name copp-system-acl-icmp
class-map type qos match-all CM-VOICE
  match dscp 46
class-map type queuing match-any c-out-q3
  match qos-group 3
policy-map type control-plane copp-system-p
  class copp-s-bgp
    police cir 5000 kbps bc 250 ms conform transmit violate drop
  class copp-s-icmp
    police cir 200 kbps bc 250 ms conform transmit violate drop
policy-map type queuing default-out-policy
  class type queuing c-out-q3
    bandwidth remaining percent 0
  class type queuing c-out-q-default
    bandwidth remaining percent 100
";

const AFTER: &str = "\
class-map type control-plane match-any copp-s-ospf
  match access-group name copp-system-acl-ospf
class-map type control-plane match-any copp-s-bgp
  match access-group name copp-system-acl-bgp
  match access-group name copp-system-acl-bgp6
class-map type control-plane match-any copp-s-icmp
  match access-group name copp-system-acl-icmp
class-map type qos match-all CM-VOICE
  match dscp 46
class-map type queuing match-any c-out-q3
  match qos-group 3
policy-map type queuing default-out-policy
  class type queuing c-out-q-default
    bandwidth remaining percent 100
  class type queuing c-out-q3
    bandwidth remaining percent 0
policy-map type control-plane copp-system-p
  class copp-s-bgp
    police cir 5000 kbps bc 250 ms conform transmit violate drop
  class copp-s-icmp
    police cir 300 kbps bc 250 ms conform transmit violate drop
";

fn keyed_stable_diff(before: &str, after: &str) -> Diff {
    let options = NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
        default: OrderPolicy::KeyedStable,
        overrides: Vec::new(),
    });
    diff_documents(&parse(before), &parse(after), options).expect("nxos configs should diff")
}

fn sorted_edit_texts(diff: &Diff) -> Vec<String> {
    let mut out = Vec::new();
    for edit in &diff.edits {
        match edit {
            Edit::Insert { lines, .. } | Edit::Delete { lines, .. } => {
                out.extend(lines.iter().map(|line| line.text.clone()));
            }
            Edit::Replace {
                old_lines,
                new_lines,
                ..
            } => {
                out.extend(old_lines.iter().map(|line| line.text.clone()));
                out.extend(new_lines.iter().map(|line| line.text.clone()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn reordered_copp_maps_pair_by_name() {
    let diff = keyed_stable_diff(BEFORE, AFTER);

    assert!(
        diff.findings
            .iter()
            .all(|f| f.code != finding_code::AMBIGUOUS_KEY_MATCH),
        "typed maps must not share a key: {:?}",
        diff.findings
    );
    assert_eq!(
        sorted_edit_texts(&diff),
        vec![
            "    police cir 200 kbps bc 250 ms conform transmit violate drop",
            "    police cir 300 kbps bc 250 ms conform transmit violate drop",
            "  match access-group name copp-system-acl-bgp6",
        ],
    );
}
