use netform_diff::{NormalizeOptions, PlanAction, PlanLineEditKind, build_plan, diff_documents};
use netform_ir::parse_generic;

#[test]
fn generates_line_edit_plan_for_single_line_replace() {
    let a = parse_generic("interface Ethernet1\n  description old\n");
    let b = parse_generic("interface Ethernet1\n  description new\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default());
    let plan = build_plan(&diff);

    assert_eq!(plan.version, "v1");
    assert_eq!(plan.findings.len(), 0);
    assert_eq!(plan.actions.len(), 1);

    match &plan.actions[0] {
        PlanAction::ApplyLineEditsUnderContext {
            context_path,
            line_edits,
        } => {
            assert_eq!(context_path.0, vec![0]);
            assert_eq!(line_edits.len(), 1);
            assert_eq!(line_edits[0].kind, PlanLineEditKind::Replace);
            assert_eq!(line_edits[0].text, "  description new");
        }
        _ => panic!("expected line-edit action"),
    }
}

#[test]
fn generates_replace_block_plan_for_multi_line_replace() {
    let a = parse_generic("interface Ethernet1\n  description old\n  mtu 9000\n");
    let b = parse_generic("router bgp 65000\n  neighbor 10.0.0.1 remote-as 65001\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default());
    let plan = build_plan(&diff);

    assert!(!plan.actions.is_empty());
    assert!(
        plan.actions
            .iter()
            .any(|a| matches!(a, PlanAction::ReplaceBlock { .. }))
    );

    let json = serde_json::to_string_pretty(&plan).expect("serialize plan");
    assert!(json.contains("\"version\""));
    assert!(json.contains("\"actions\""));
    assert!(json.contains("\"findings\""));
}
