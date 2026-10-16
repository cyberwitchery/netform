use std::collections::HashMap;
use std::fs;

use netform_diff::{
    Diff, DiffLine, Edit, NormalizationStep, NormalizeOptions, OrderPolicy, OrderPolicyConfig,
    Plan, PlanAction, PlanLineEdit, PlanLineEditKind, build_comparison_view, build_plan,
    diff_documents,
};
use netform_ir::{Document, parse_generic};
use proptest::prelude::*;

type Parse = fn(&str) -> Document;

#[derive(Debug, Clone)]
struct Tree {
    original: String,
    text: String,
    children: Vec<Tree>,
}

impl Tree {
    /// the tree's normalized text with every level's children sorted.
    fn canonical(&self) -> String {
        let mut children = self
            .children
            .iter()
            .map(Tree::canonical)
            .collect::<Vec<_>>();
        children.sort();
        format!("{}{{{}}}", self.text, children.join("\u{1}"))
    }
}

/// parses `text` and nests its comparison lines by path.
fn trees(text: &str, parse: Parse, options: &NormalizeOptions) -> Vec<Tree> {
    fn insert(roots: &mut Vec<Tree>, path: &[usize], node: Tree, depth: usize) {
        let siblings = roots;
        if depth + 1 == path.len() {
            siblings.push(node);
        } else {
            let parent = siblings.last_mut().expect("a line's parent precedes it");
            insert(&mut parent.children, path, node, depth + 1);
        }
    }

    let mut roots = Vec::new();
    for line in build_comparison_view(&parse(text), options).lines {
        let node = Tree {
            original: line.original,
            text: line.normalized,
            children: Vec::new(),
        };
        insert(&mut roots, &line.path.0, node, 0);
    }
    roots
}

struct Node {
    original: String,
    text: String,
    parent: usize,
    children: Vec<usize>,
}

/// applies plan actions to the left document, addressing nodes by their original paths.
struct Applier<'a> {
    nodes: Vec<Node>,
    by_path: HashMap<Vec<usize>, usize>,
    replaced_from: HashMap<(usize, String), Vec<String>>,
    parse: Parse,
    options: &'a NormalizeOptions,
}

impl<'a> Applier<'a> {
    fn new(left: &str, diff: &Diff, parse: Parse, options: &'a NormalizeOptions) -> Self {
        let mut applier = Self {
            nodes: vec![Node {
                original: String::new(),
                text: String::new(),
                parent: 0,
                children: Vec::new(),
            }],
            by_path: HashMap::from([(Vec::new(), 0)]),
            replaced_from: HashMap::new(),
            parse,
            options,
        };
        for line in build_comparison_view(&parse(left), options).lines {
            let parent = applier.by_path[&line.path.0[..line.path.0.len() - 1]];
            let index = applier.push(parent, line.original, line.normalized);
            applier.by_path.insert(line.path.0, index);
        }
        // a plan's replace carries only the new text; the diff says which line it replaces.
        for edit in &diff.edits {
            if let Edit::Replace {
                old_lines,
                new_lines,
                ..
            } = edit
                && let ([old], [new]) = (old_lines.as_slice(), new_lines.as_slice())
            {
                let context = applier.by_path[&old.path.0[..old.path.0.len() - 1]];
                applier
                    .replaced_from
                    .entry((context, new.text.clone()))
                    .or_default()
                    .push(old.text.clone());
            }
        }
        applier
    }

    fn push(&mut self, parent: usize, original: String, text: String) -> usize {
        self.nodes.push(Node {
            original,
            text,
            parent,
            children: Vec::new(),
        });
        let index = self.nodes.len() - 1;
        self.nodes[parent].children.push(index);
        index
    }

    fn graft(&mut self, parent: usize, tree: Tree) -> usize {
        let index = self.push(parent, tree.original, tree.text);
        for child in tree.children {
            self.graft(index, child);
        }
        index
    }

    fn tree(&self, index: usize) -> Tree {
        let node = &self.nodes[index];
        Tree {
            original: node.original.clone(),
            text: node.text.clone(),
            children: node
                .children
                .iter()
                .map(|&child| self.tree(child))
                .collect(),
        }
    }

    fn node_at(&self, path: &netform_ir::Path) -> Result<usize, String> {
        self.by_path
            .get(&path.0)
            .copied()
            .ok_or_else(|| format!("path {:?} does not exist in the left document", path.0))
    }

    fn fragment<'t>(&self, texts: impl IntoIterator<Item = &'t String>) -> Vec<Tree> {
        let text = texts
            .into_iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        trees(&text, self.parse, self.options)
    }

    fn apply(&mut self, plan: &Plan) -> Result<(), String> {
        for action in &plan.actions {
            match action {
                PlanAction::ReplaceBlock {
                    target_path,
                    intended_lines,
                    ..
                } => {
                    let target = self.node_at(target_path)?;
                    let parent = self.nodes[target].parent;
                    let fragment = self.fragment(intended_lines);
                    self.nodes[parent].children.retain(|&child| child != target);
                    for tree in fragment {
                        self.graft(parent, tree);
                    }
                }
                PlanAction::ApplyLineEditsUnderContext {
                    context_path,
                    line_edits,
                } => {
                    let context = self.node_at(context_path)?;
                    for run in line_edits.chunk_by(|a, b| a.kind == b.kind) {
                        self.apply_run(context, run)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn apply_run(&mut self, context: usize, run: &[PlanLineEdit]) -> Result<(), String> {
        let texts = run.iter().map(|edit| &edit.text);
        match run[0].kind {
            PlanLineEditKind::Insert => {
                for tree in self.fragment(texts) {
                    self.graft(context, tree);
                }
            }
            PlanLineEditKind::Delete => {
                for tree in self.fragment(texts) {
                    let wanted = tree.canonical();
                    let position = self.nodes[context]
                        .children
                        .iter()
                        .position(|&child| self.tree(child).canonical() == wanted)
                        .ok_or_else(|| format!("no `{}` to delete", tree.original))?;
                    self.nodes[context].children.remove(position);
                }
            }
            PlanLineEditKind::Replace => {
                for edit in run {
                    let olds = self
                        .replaced_from
                        .get_mut(&(context, edit.text.clone()))
                        .ok_or_else(|| format!("no line for replace `{}`", edit.text))?;
                    let (child, old) = self.nodes[context]
                        .children
                        .iter()
                        .find_map(|&child| {
                            let original = &self.nodes[child].original;
                            let old = olds.iter().position(|old| old == original)?;
                            Some((child, old))
                        })
                        .ok_or_else(|| format!("no line for replace `{}`", edit.text))?;
                    olds.swap_remove(old);
                    let new = self
                        .fragment([&edit.text])
                        .pop()
                        .ok_or_else(|| format!("replace `{}` is not a line", edit.text))?;
                    self.nodes[child].original = new.original;
                    self.nodes[child].text = new.text;
                }
            }
        }
        Ok(())
    }
}

/// diffs `left` against `right`, applies the plan to `left`, and compares the result with `right`.
fn check_plan(left: &str, right: &str, parse: Parse, options: &NormalizeOptions) {
    let diff = diff_documents(&parse(left), &parse(right), options.clone()).expect("diff");
    let plan = build_plan(&diff);
    let plan_json = serde_json::to_string_pretty(&plan).expect("serialize plan");
    assert!(
        plan.findings.is_empty(),
        "plan has findings\nleft:\n{left}\nright:\n{right}\nplan: {plan_json}"
    );

    let mut applier = Applier::new(left, &diff, parse, options);
    if let Err(error) = applier.apply(&plan) {
        panic!("{error}\nleft:\n{left}\nright:\n{right}\nplan: {plan_json}");
    }
    let applied = applier.tree(0);
    let expected = Tree {
        original: String::new(),
        text: String::new(),
        children: trees(right, parse, options),
    };
    assert_eq!(
        applied.canonical(),
        expected.canonical(),
        "applying the plan to the left document does not give the right one\n\
         left:\n{left}\nright:\n{right}\nplan: {plan_json}"
    );
}

fn options_for(policy: OrderPolicy) -> NormalizeOptions {
    NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
        default: policy,
        overrides: Vec::new(),
    })
}

const POLICIES: [OrderPolicy; 3] = [
    OrderPolicy::Ordered,
    OrderPolicy::Unordered,
    OrderPolicy::KeyedStable,
];

#[test]
fn line_moved_between_blocks_is_planned_under_each_block() {
    let left = "hostname x\ninterface Ethernet1\n   description srv1\n   mtu 9000\ninterface Ethernet2\n   shutdown\n";
    let right =
        "interface Ethernet1\n   mtu 9000\ninterface Ethernet2\n   shutdown\n   description srv1\n";
    for policy in POLICIES {
        check_plan(left, right, parse_generic, &options_for(policy));
    }
}

#[test]
fn renamed_block_is_planned_with_its_body() {
    let left = "hostname x\ninterface Ethernet1\n   description srv1\n   mtu 9000\n";
    let right = "hostname x\ninterface Ethernet9\n   description srv1\n   mtu 9000\n";
    for policy in POLICIES {
        check_plan(left, right, parse_generic, &options_for(policy));
    }
}

#[test]
fn new_nested_block_is_planned_whole_under_its_parent() {
    let left = "router bgp 1\n   neighbor 1.1.1.1 remote-as 2\n";
    let right = "router bgp 1\n   neighbor 1.1.1.1 remote-as 2\n   address-family ipv4\n      network 10.0.0.0/8\n      neighbor 1.1.1.1 activate\n";
    for policy in POLICIES {
        check_plan(left, right, parse_generic, &options_for(policy));
    }
}

#[test]
fn edited_twin_block_is_planned_before_its_twin_is_deleted() {
    let left = "router bgp 1\n   address-family ipv4\n      network 10.1.0.0/16\n   address-family ipv4\n      network 10.1.0.0/16\n";
    let right = "router bgp 1\n   address-family ipv6\n      network 2001:db8::/32\n   address-family ipv4\n      network 10.1.0.0/16\n      network 10.2.0.0/16\n";
    for policy in POLICIES {
        check_plan(left, right, parse_generic, &options_for(policy));
    }
}

#[test]
fn edited_route_policy_else_branch_is_planned_before_its_twin_is_deleted() {
    let parse = netform_dialects::find("iosxr")
        .expect("iosxr dialect")
        .parse;
    let left = "route-policy X\n  if destination in A then\n    drop\n  else\n    pass\n  endif\n  if destination in B then\n    drop\n  else\n    pass\n  endif\nend-policy\n";
    let right = "route-policy X\n  set med 10\n  if destination in A then\n    drop\n  else\n    set local-preference 200\n    pass\n  endif\nend-policy\n";
    for policy in POLICIES {
        check_plan(left, right, parse, &options_for(policy));
    }
}

#[test]
fn replaces_with_the_same_new_text_apply_to_their_own_lines() {
    let left =
        "interface Ethernet1\n  description x\ninterface Ethernet2\n  description x\n  mtu 1500\n";
    let right =
        "interface Ethernet1\n  mtu 9000\ninterface Ethernet2\n  description x\n  mtu 9000\n";
    check_plan(
        left,
        right,
        parse_generic,
        &options_for(OrderPolicy::Ordered),
    );
}

/// each edited line's text with the kind of the edit it is in, sorted.
fn edited_lines(diff: &Diff) -> Vec<(&'static str, &str)> {
    let mut lines: Vec<_> = diff
        .edits
        .iter()
        .flat_map(|edit| {
            let (kind, lines): (_, Vec<&DiffLine>) = match edit {
                Edit::Insert { lines, .. } => ("insert", lines.iter().collect()),
                Edit::Delete { lines, .. } => ("delete", lines.iter().collect()),
                Edit::Replace {
                    old_lines,
                    new_lines,
                    ..
                } => ("replace", old_lines.iter().chain(new_lines).collect()),
            };
            lines
                .into_iter()
                .map(move |line| (kind, line.text.as_str()))
        })
        .collect();
    lines.sort_unstable();
    lines
}

#[test]
fn an_earlier_ios_family_sibling_is_removed_and_added_on_its_own() {
    let rows: &[(&str, Parse, &str, &str)] = &[
        (
            "isakmp policy",
            netform_dialects::iosxe::parse,
            "crypto isakmp policy 10\n encryption aes\n",
            "crypto isakmp policy 20\n encryption 3des\n",
        ),
        (
            "isakmp key",
            netform_dialects::iosxe::parse,
            "crypto isakmp key K1 address 192.0.2.1\n",
            "crypto isakmp key K2 address 192.0.2.2\n",
        ),
        (
            "crypto map",
            netform_dialects::iosxe::parse,
            "crypto map CMAP 10 ipsec-isakmp\n set peer 192.0.2.1\n",
            "crypto map CMAP 20 ipsec-isakmp\n set peer 192.0.2.2\n",
        ),
        (
            "sa lifetime",
            netform_dialects::iosxe::parse,
            "crypto ipsec security-association lifetime seconds 3600\n",
            "crypto ipsec security-association lifetime kilobytes 4608000\n",
        ),
        (
            "ntp vrf",
            netform_dialects::iosxr::parse,
            "ntp server vrf MGMT 10.0.0.1\n",
            "ntp server vrf MGMT 10.0.0.2\n",
        ),
        (
            "prefix-list",
            netform_dialects::nxos::parse,
            "ip prefix-list PL seq 5 permit 10.0.0.0/8\n",
            "ip prefix-list PL seq 10 permit 192.168.0.0/16\n",
        ),
        (
            "ipv6 prefix-list",
            netform_dialects::iosxe::parse,
            "ipv6 prefix-list V6 seq 5 permit 2001:db8::/32\n",
            "ipv6 prefix-list V6 seq 10 permit 2001:db9::/32\n",
        ),
        (
            "community-list",
            netform_dialects::iosxe::parse,
            "ip community-list standard CL permit 65000:1\n",
            "ip community-list standard CL permit 65000:2\n",
        ),
        (
            "monitor session",
            netform_dialects::iosxe::parse,
            "monitor session 1 source interface GigabitEthernet1/0/1 both\n",
            "monitor session 1 destination interface GigabitEthernet1/0/2\n",
        ),
        (
            "spanning-tree vlan",
            netform_dialects::iosxe::parse,
            "spanning-tree vlan 10 priority 4096\n",
            "spanning-tree vlan 10 hello-time 1\n",
        ),
        (
            "route dotted mask",
            netform_dialects::iosxe::parse,
            "ip route 10.0.0.0 255.0.0.0 Null0\n",
            "ip route 10.0.0.0 255.255.0.0 192.0.2.1\n",
        ),
        (
            "route ecmp",
            netform_dialects::eos::parse,
            "ip route 10.0.0.0/8 192.0.2.1\n",
            "ip route 10.0.0.0/8 192.0.2.2\n",
        ),
        (
            "ipv6 route ecmp",
            netform_dialects::nxos::parse,
            "ipv6 route 2001:db8::/32 2001:db8:ffff::1\n",
            "ipv6 route 2001:db8::/32 2001:db8:ffff::2\n",
        ),
        (
            "route static bfd",
            netform_dialects::iosxe::parse,
            "ip route static bfd GigabitEthernet0/0 192.0.2.1\n",
            "ip route static bfd GigabitEthernet0/1 192.0.2.5\n",
        ),
    ];
    for &(family, parse, earlier, kept) in rows {
        let with = format!("hostname r1\n{earlier}{kept}");
        let without = format!("hostname r1\n{kept}");
        let mut statement: Vec<&str> = earlier.lines().collect();
        statement.sort_unstable();
        for policy in POLICIES {
            let options = options_for(policy);
            for (left, right, kind) in [(&with, &without, "delete"), (&without, &with, "insert")] {
                let diff =
                    diff_documents(&parse(left), &parse(right), options.clone()).expect("diff");
                assert_eq!(
                    edited_lines(&diff),
                    statement
                        .iter()
                        .map(|line| (kind, *line))
                        .collect::<Vec<_>>(),
                    "{family}, {policy:?}"
                );
                check_plan(left, right, parse, &options);
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct Fixture {
    name: String,
    #[serde(default = "generic")]
    dialect: String,
    intended: String,
    actual: String,
    normalization_steps: Vec<NormalizationStep>,
    order_policy: OrderPolicyConfig,
}

fn generic() -> String {
    "generic".to_string()
}

#[test]
fn plans_apply_to_the_left_side_of_every_fixture() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures");
    let mut checked = 0;
    for entry in fs::read_dir(fixtures).expect("read fixtures") {
        let path = entry.expect("fixture entry").path();
        let fixture: Fixture =
            serde_json::from_str(&fs::read_to_string(&path).expect("read fixture"))
                .expect("parse fixture");
        let parse: Parse = match fixture.dialect.as_str() {
            "generic" => parse_generic,
            name => {
                netform_dialects::find(name)
                    .unwrap_or_else(|| panic!("{}: unknown dialect {name}", fixture.name))
                    .parse
            }
        };
        let steps = fixture.normalization_steps;
        let configs = POLICIES
            .map(|policy| OrderPolicyConfig {
                default: policy,
                overrides: Vec::new(),
            })
            .into_iter()
            .chain([fixture.order_policy]);
        for config in configs {
            let options = NormalizeOptions::new(steps.clone()).with_order_policy(config);
            check_plan(&fixture.intended, &fixture.actual, parse, &options);
            check_plan(&fixture.actual, &fixture.intended, parse, &options);
            checked += 2;
        }
    }
    assert!(checked > 0, "no fixtures found");
}

#[derive(Debug, Clone)]
struct Generated {
    text: &'static str,
    children: Vec<Generated>,
}

fn config_strategy() -> impl Strategy<Value = String> {
    let line = prop::sample::select(vec![
        "description srv1",
        "description srv2",
        "mtu 9000",
        "shutdown",
        "neighbor 1.1.1.1",
    ]);
    let header = prop::sample::select(vec![
        "interface Ethernet1",
        "interface Ethernet2",
        "address-family ipv4",
        "router bgp 1",
    ]);
    let leaf = line.prop_map(|text| Generated {
        text,
        children: Vec::new(),
    });
    let tree = leaf.prop_recursive(3, 24, 4, move |inner| {
        (header.clone(), prop::collection::vec(inner, 1..4))
            .prop_map(|(text, children)| Generated { text, children })
    });

    prop::collection::vec(tree, 0..5).prop_map(|roots| {
        let mut out = String::new();
        let mut stack = roots.iter().rev().map(|root| (root, 0)).collect::<Vec<_>>();
        while let Some((node, depth)) = stack.pop() {
            out.push_str(&"  ".repeat(depth));
            out.push_str(node.text);
            out.push('\n');
            stack.extend(node.children.iter().rev().map(|child| (child, depth + 1)));
        }
        out
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn generated_plans_apply_to_the_left_side(
        left in config_strategy(),
        right in config_strategy(),
        policy in prop::sample::select(POLICIES.to_vec()),
    ) {
        check_plan(&left, &right, parse_generic, &options_for(policy));
    }
}
