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
    for (left, right) in [
        (
            "hostname x\ninterface Ethernet1\n   description srv1\n   mtu 9000\ninterface Ethernet2\n   shutdown\n",
            "interface Ethernet1\n   mtu 9000\ninterface Ethernet2\n   shutdown\n   description srv1\n",
        ),
        (
            "interface Ethernet2\n   description srv1\nvrf A\n   !\n",
            "vrf A\n   description srv1\n",
        ),
    ] {
        for policy in POLICIES {
            check_plan(left, right, parse_generic, &options_for(policy));
        }
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
fn vdom_tables_are_planned_under_their_vdom() {
    let parse = netform_dialects::fortios::parse;
    let left = "config vdom\nedit root\nconfig firewall address\n    edit \"web\"\n        set subnet 10.0.0.10 255.255.255.255\n    next\nend\nnext\nedit dmz\nconfig firewall address\n    edit \"db\"\n        set subnet 10.1.0.20 255.255.255.255\n    next\nend\nnext\nend\n";
    let right = "config vdom\nedit root\nnext\nedit dmz\nconfig firewall address\n    edit \"db\"\n        set subnet 10.1.0.21 255.255.255.255\n    next\nend\nnext\nend\n";
    for policy in POLICIES {
        check_plan(left, right, parse, &options_for(policy));
    }
    let diff = diff_documents(
        &parse(left),
        &parse(right),
        options_for(OrderPolicy::Ordered),
    )
    .expect("diff");
    let contexts: Vec<Vec<usize>> = build_plan(&diff)
        .actions
        .iter()
        .map(|action| match action {
            PlanAction::ApplyLineEditsUnderContext { context_path, .. } => context_path.0.clone(),
            PlanAction::ReplaceBlock { target_path, .. } => target_path.0.clone(),
        })
        .collect();
    assert_eq!(contexts, [vec![0, 1, 0, 0], vec![0, 0]]);
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
fn an_earlier_sibling_is_removed_and_added_on_its_own() {
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
            "route distance",
            netform_dialects::eos::parse,
            "ip route 4.4.4.4/32 Ethernet1\n",
            "ip route 4.4.4.4/32 Ethernet1 7\n",
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
        (
            "junos ecmp next-hop",
            netform_dialects::junos::parse,
            "set routing-options static route 10.0.0.0/8 next-hop 192.0.2.1\n",
            "set routing-options static route 10.0.0.0/8 next-hop 192.0.2.2\n",
        ),
        (
            "junos authentication-order",
            netform_dialects::junos::parse,
            "set system services telnet authentication-order tacplus\n",
            "set system services telnet authentication-order password\n",
        ),
        (
            "junos bfd-liveness-detection",
            netform_dialects::junos::parse,
            "set routing-options static route 10.0.0.0/8 bfd-liveness-detection minimum-interval 900\n",
            "set routing-options static route 10.0.0.0/8 bfd-liveness-detection no-adaptation\n",
        ),
        (
            "same-header block",
            parse_generic,
            "router bgp 1\n network 10.1.0.0/16\n",
            "router bgp 1\n network 10.2.0.0/16\n",
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

#[test]
fn an_only_child_is_removed_and_added_on_its_own() {
    let rows: &[(&str, Parse, &str, &str, &str)] = &[
        (
            "interface",
            netform_dialects::iosxe::parse,
            "hostname r1\n!\ninterface Gi0/1\n shutdown\n!\ninterface Gi0/2\n description x\n!\nend\n",
            "hostname r1\n!\ninterface Gi0/1\n!\ninterface Gi0/2\n description x\n!\nend\n",
            " shutdown",
        ),
        (
            "nested address-family",
            netform_dialects::iosxe::parse,
            "router bgp 1\n neighbor 1.1.1.1 remote-as 2\n address-family ipv4\n  neighbor 1.1.1.1 activate\n exit-address-family\n!\n",
            "router bgp 1\n neighbor 1.1.1.1 remote-as 2\n address-family ipv4\n exit-address-family\n!\n",
            "  neighbor 1.1.1.1 activate",
        ),
        (
            "generic",
            parse_generic,
            "hostname h\n  foo\n",
            "hostname h\n",
            "  foo",
        ),
        (
            "comment-only body",
            netform_dialects::eos::parse,
            "vrf instance A\n   !\n   rd 1:1\nhostname x\n",
            "vrf instance A\n   !\nhostname x\n",
            "   rd 1:1",
        ),
        (
            "junos comment-only body",
            netform_dialects::junos::parse,
            "system {\n    /* note */\n    host-name r1;\n}\n",
            "system {\n    /* note */\n}\n",
            "    host-name r1;",
        ),
        (
            "junos comment region",
            netform_dialects::junos::parse,
            "system {\n    /* note\n       more */\n    host-name r1;\n}\n",
            "system {\n    /* note\n       more */\n}\n",
            "    host-name r1;",
        ),
        (
            "fortios comment-only body",
            netform_dialects::fortios::parse,
            "config system global\n    # note\n    set hostname r1\nend\n",
            "config system global\n    # note\nend\n",
            "    set hostname r1",
        ),
        (
            "junos empty block",
            netform_dialects::junos::parse,
            "interfaces {\n    ge-0/0/0 {\n        disable;\n    }\n}\n",
            "interfaces {\n    ge-0/0/0 {\n    }\n}\n",
            "        disable;",
        ),
        (
            "fortios empty config",
            netform_dialects::fortios::parse,
            "config system global\n    set hostname r1\nend\n",
            "config system global\nend\n",
            "    set hostname r1",
        ),
        (
            "fortios empty edit",
            netform_dialects::fortios::parse,
            "config firewall address\n    edit \"a\"\n        set subnet 10.0.0.0 255.0.0.0\n    next\nend\n",
            "config firewall address\n    edit \"a\"\n    next\nend\n",
            "        set subnet 10.0.0.0 255.0.0.0",
        ),
        (
            "iosxr empty set",
            netform_dialects::iosxr::parse,
            "rd-set RS\n  65000:1\nend-set\n!\n",
            "rd-set RS\nend-set\n!\n",
            "  65000:1",
        ),
    ];
    for &(family, parse, with, without, child) in rows {
        for policy in POLICIES {
            let options = options_for(policy);
            for (left, right, kind) in [(with, without, "delete"), (without, with, "insert")] {
                let diff =
                    diff_documents(&parse(left), &parse(right), options.clone()).expect("diff");
                assert_eq!(edited_lines(&diff), [(kind, child)], "{family}, {policy:?}");
                assert!(
                    !diff
                        .findings
                        .iter()
                        .any(|finding| finding.code == "diff_unreliable_region"),
                    "{family}, {policy:?}: {:?}",
                    diff.findings
                );
                check_plan(left, right, parse, &options);
            }
        }
    }
}

#[test]
fn a_changed_line_between_reshuffled_same_key_siblings_is_planned() {
    let left = "set routing-options static route 10.0.0.0/8 next-hop 192.0.2.1\n\
                set routing-options static route 10.0.0.0/8 bfd-liveness-detection minimum-interval 300\n\
                set routing-options static route 10.0.0.0/8 next-hop 192.0.2.3\n";
    let right = "set routing-options static route 10.0.0.0/8 next-hop 192.0.2.2\n\
                 set routing-options static route 10.0.0.0/8 next-hop 192.0.2.1\n\
                 set routing-options static route 10.0.0.0/8 bfd-liveness-detection minimum-interval 900\n";
    let parse = netform_dialects::junos::parse;
    for policy in POLICIES {
        check_plan(left, right, parse, &options_for(policy));
        check_plan(right, left, parse, &options_for(policy));
    }
}

#[test]
fn an_unchanged_sibling_stays_paired_whichever_file_comes_first() {
    type Edits = &'static [(&'static str, &'static str)];
    let rows: &[(&str, Parse, &str, &str, Edits)] = &[
        (
            "route beside ntp",
            netform_dialects::iosxe::parse,
            "ip route 10.0.0.0 255.0.0.0 192.0.2.1 name X\nntp server 192.0.2.1\n",
            "ntp server 192.0.2.1\nip route 10.0.0.0 255.0.0.0 192.0.2.1 name Y\n",
            &[
                ("delete", "ip route 10.0.0.0 255.0.0.0 192.0.2.1 name X"),
                ("insert", "ip route 10.0.0.0 255.0.0.0 192.0.2.1 name Y"),
            ],
        ),
        (
            "route beside ntp",
            netform_dialects::eos::parse,
            "ip route 10.0.0.0 255.0.0.0 192.0.2.1 name X\nntp server 192.0.2.1\n",
            "ntp server 192.0.2.1\nip route 10.0.0.0 255.0.0.0 192.0.2.1 name Y\n",
            &[
                ("delete", "ip route 10.0.0.0 255.0.0.0 192.0.2.1 name X"),
                ("insert", "ip route 10.0.0.0 255.0.0.0 192.0.2.1 name Y"),
            ],
        ),
        (
            "two key matches beat one identical",
            netform_dialects::iosxe::parse,
            "ip route 10.0.0.0 255.0.0.0 192.0.2.1 name X\nntp server 192.0.2.1\nhostname r1\n",
            "hostname r1\nip route 10.0.0.0 255.0.0.0 192.0.2.1 name Y\nntp server 192.0.2.1\n",
            &[
                ("insert", "hostname r1"),
                ("delete", "hostname r1"),
                ("replace", "ip route 10.0.0.0 255.0.0.0 192.0.2.1 name X"),
                ("replace", "ip route 10.0.0.0 255.0.0.0 192.0.2.1 name Y"),
            ],
        ),
        (
            "an identical block outweighs an identical leaf",
            netform_dialects::iosxe::parse,
            "interface Gi0/1\n description x\n shutdown\nhostname r1\n",
            "hostname r1\ninterface Gi0/1\n description x\n shutdown\n",
            &[("delete", "hostname r1"), ("insert", "hostname r1")],
        ),
        (
            "a block sharing two lines outweighs an identical leaf",
            netform_dialects::iosxe::parse,
            "interface Gi0/1\n description x\n shutdown\nhostname r1\n",
            "hostname r1\ninterface Gi0/1\n description y\n shutdown\n",
            &[
                ("delete", "hostname r1"),
                ("insert", "hostname r1"),
                ("replace", " description x"),
                ("replace", " description y"),
            ],
        ),
        (
            "a block whose children only moved does not outweigh an unchanged one",
            netform_dialects::iosxe::parse,
            "interface Gi1\n description one\n mtu 1500\n no shutdown\ninterface Gi2\n description two\n mtu 9000\n",
            "interface Gi2\n description two\n mtu 9000\ninterface Gi1\n no shutdown\n mtu 1500\n description one\n",
            &[
                ("delete", "interface Gi1"),
                ("delete", " description one"),
                ("delete", " mtu 1500"),
                ("delete", " no shutdown"),
                ("insert", "interface Gi1"),
                ("insert", " no shutdown"),
                ("insert", " mtu 1500"),
                ("insert", " description one"),
            ],
        ),
        (
            "a block whose children only moved does not outweigh an unchanged one",
            netform_dialects::junos::parse,
            "interfaces {\n    ge-0/0/1 {\n        description one;\n        mtu 1500;\n        vlan-tagging;\n    }\n    ge-0/0/2 {\n        description two;\n        mtu 9000;\n    }\n}\n",
            "interfaces {\n    ge-0/0/2 {\n        description two;\n        mtu 9000;\n    }\n    ge-0/0/1 {\n        vlan-tagging;\n        mtu 1500;\n        description one;\n    }\n}\n",
            &[
                ("delete", "    ge-0/0/1 {"),
                ("delete", "        description one;"),
                ("delete", "        mtu 1500;"),
                ("delete", "        vlan-tagging;"),
                ("delete", "    }"),
                ("insert", "    ge-0/0/1 {"),
                ("insert", "        vlan-tagging;"),
                ("insert", "        mtu 1500;"),
                ("insert", "        description one;"),
                ("insert", "    }"),
            ],
        ),
    ];
    let options = options_for(OrderPolicy::Ordered);
    for &(case, parse, left, right, edits) in rows {
        let flipped = edits
            .iter()
            .map(|&(kind, line)| match kind {
                "delete" => ("insert", line),
                "insert" => ("delete", line),
                _ => (kind, line),
            })
            .collect::<Vec<_>>();
        for (from, to, mut expected) in [(left, right, edits.to_vec()), (right, left, flipped)] {
            expected.sort_unstable();
            let diff = diff_documents(&parse(from), &parse(to), options.clone()).expect("diff");
            assert_eq!(edited_lines(&diff), expected, "{case}: {from:?} -> {to:?}");
            check_plan(from, to, parse, &options);
        }
    }
}

#[test]
fn reordered_same_key_siblings_report_nothing() {
    let rows: &[(&str, Parse, &str, &str)] = &[
        (
            "junos ecmp next-hops",
            netform_dialects::junos::parse,
            "set routing-options static route 10.0.0.0/8 next-hop 192.0.2.1\n\
             set routing-options static route 10.0.0.0/8 next-hop 192.0.2.2\n",
            "set routing-options static route 10.0.0.0/8 next-hop 192.0.2.2\n\
             set routing-options static route 10.0.0.0/8 next-hop 192.0.2.1\n",
        ),
        (
            "fortios repeated field",
            netform_dialects::fortios::parse,
            "config system interface\n    edit \"port1\"\n        set member \"a\"\n        set member \"b\"\n        set member \"c\"\n    next\nend\n",
            "config system interface\n    edit \"port1\"\n        set member \"c\"\n        set member \"a\"\n        set member \"b\"\n    next\nend\n",
        ),
    ];
    for &(case, parse, left, right) in rows {
        for policy in [OrderPolicy::KeyedStable, OrderPolicy::Unordered] {
            let diff =
                diff_documents(&parse(left), &parse(right), options_for(policy)).expect("diff");
            assert!(!diff.has_changes, "{case}, {policy:?}: {:?}", diff.edits);
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
