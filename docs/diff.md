# diff model

`netform_diff` computes deterministic edits from two `netform_ir::Document` values.

top-level drift is exposed as `Diff.has_changes`.

## normalization

`NormalizeOptions` uses an explicit ordered step pipeline. `config-diff` turns on one step per flag
and runs them in this order, whatever the order of the flags:

- `ignore_comments` (`--ignore-comments`): drops the lines the dialect classifies as comments.
- `ignore_blank_lines` (`--ignore-blank-lines`): drops lines that are empty or only whitespace.
- `collapse_internal_whitespace` (`--normalize-whitespace`): joins a line's words with single
  spaces, which also drops trailing whitespace. indentation is kept.
- `trim_trailing_whitespace` (`--trim-trailing-whitespace`): drops trailing whitespace.
- `normalize_leading_whitespace` (`--normalize-leading-whitespace`): rewrites indentation as
  spaces, a tab counting as four.

applied steps are recorded in `Diff.normalization_steps`. normalized text is only compared: reports
and plans print each line as written.

no step changes or drops literal text, such as the body lines of a multi-line banner or the lines a
multi-line quoted value continues on; the `banner` line itself is normalized. on FortiOS and Junos
the whitespace steps also leave a quoted value as it is on a line it shares with configuration;
whitespace between words and after a closing quote is still normalized. the other dialects treat a
quote as plain text. with this FortiOS `a.conf`, and a `b.conf` that has single spaces in both `set`
lines, only the quoted value is reported:

```text
config firewall policy
    edit 1
        set name  "web"
        set comments "allow  web"
    next
end
```

```text
$ config-diff --normalize-whitespace a.conf b.conf
--- a.conf
+++ b.conf
@@ replace 1 line(s) at key 0x4f67837938ed09fa -> 1 line(s) at key 0x4f67837938ed09fa @@
-         set comments "allow  web"
+         set comments "allow web"
```

## order policy

ordering is explicit and reproducible through `OrderPolicyConfig`: a default (`--order-policy`,
`ordered` unless set) and per-context overrides (`--policy-override`). resolved policy config is
emitted in `Diff.order_policy`.

a policy governs one level: the top-level lines, or the children of one block. a line's key is the
dialect's key hint where it has one (FortiOS `set name` keys on `name`, EOS `ntp server` on its
address), otherwise its normalized text, and it is scoped to the line's parent block.

- `ordered` (the default): siblings pair on their key, in order, and a delete and an insert at the
  same place are one replace, so a line changed in place is a replace.
- `unordered`: siblings pair on their normalized text, blocks on their header, in any order. a line
  whose text changes is a delete and an insert.
- `keyed-stable`: siblings pair on their key, in any order. a line that keeps its key but changes
  its value is a replace; any other change is a delete and an insert.

what each reports, the same at the top level and inside blocks:

| change | `ordered` | `unordered` | `keyed-stable` |
| --- | --- | --- | --- |
| siblings swap places | the moved lines, deleted and inserted | nothing | nothing |
| a line moves into a sibling block | deleted under one, inserted under the other | same | same |
| a repeated identical line gains or loses a copy | one insert or one delete | same | same |

a key that appears more than once on both sides also gets an `ambiguous_key_match` finding. keys are
counted within one parent block, except a block header's key hint, which is counted across the whole
file, so FortiOS `edit 1` under both `config firewall policy` and `config firewall address` gets the
finding under every policy.

at the top level, a sibling that an in-order pass cannot pair on its key (on its text under
`unordered`), that is an added, removed, moved or re-keyed line, is itself flagged
`diff_unreliable_region` when that leaves an edit: one finding per run of such siblings, at the
run's first line. `ordered` decides run by run, `unordered` and `keyed-stable` for all such siblings
at once, so there a reorder next to an edit is flagged too. a line that keeps its key is paired and
not flagged: EOS `ntp server 10.0.0.1 prefer` changed to `ntp server 10.0.0.1` is one replace with
no finding under `ordered` and `keyed-stable`, and a flagged delete and insert under `unordered`.
inside a block nothing is flagged this way.

which of several candidate siblings pairs with which, and so which lines a change is reported
against, is not specified yet and can change between releases.

## policy overrides

`config-diff --policy-override PATH:POLICY` sets the policy for the children of one node and for
every level below them. PATH is the node's path in the left-hand file as `--json` prints it, joined
with dots: `1` is the second top-level node, `1.0` its first child. paths count every node,
comments and blank lines included. an empty PATH (`:unordered`) names the top level, so it covers
the whole file.

the flag may be repeated. each level takes the policy of the longest PATH that names it or one of
its ancestors (of the first flag, if that PATH is given twice), and `--order-policy` where none
does. under an `ordered` level, though, a run of siblings that do not pair is compared under the
policy of its first sibling's own path, so an override on a line with no children can change how the
run it starts is reported; like the pairing rules, this is not specified yet.

with this `a.cfg`, and a `b.cfg` that swaps the interface's two lines, `ordered` reports the swap
and an override on `interface Ethernet1` (path `1`) ignores it:

```text
hostname r1
interface Ethernet1
 description uplink
 shutdown
```

```text
$ config-diff --dialect eos a.cfg b.cfg
--- a.cfg
+++ b.cfg
@@ delete 1 line(s) at key 0xb212372d72eef0ed @@
-  description uplink
@@ insert 1 line(s) at key 0xb212372d72eef0ed @@
+  description uplink
$ config-diff --dialect eos --policy-override 1:unordered a.cfg b.cfg
$ echo $?
0
```

## comparison view

each comparable line carries:

- normalized text (for matching)
- original text (for reporting)
- `content_key` (semantic hash key)
- `occurrence_key` (hash of `content_key` and the line's ordinal among the document's lines that
  share it, which tells duplicate lines apart)
- `Path` (node path)
- `Span` (line + byte offsets)
- trivia classification
- optional dialect-provided `key_hint` used for keyed-stable matching when available

both keys are hashes that can change between releases, so compare them only between outputs of the
same version.

## edits

`Diff.edits` holds grouped edit-script operations:

- `Insert`
- `Delete`
- `Replace`

every edit includes both-side anchors where available:

- `left_anchor { path, span }`
- `right_anchor { path, span }`

an `Insert` below the top level carries as its `left_anchor` the header of the left-side block its
lines go into, or the left-side line they become the first children of. it has none at the top
level, nor when that block exists only on the right.

changed lines also carry path/span references for diagnostics.

each edit also carries the `occurrence_key` of its first line: `at_key` on `Insert` and `Delete`,
`old_at_key` and `new_at_key` (of the first old and the first new line) on `Replace`. the unified
report prints them as `at key 0x…`. like the keys above they can change between releases.

## findings

`Diff.findings` and the plan's `findings` carry explicit uncertainty/warning signals with stable
codes. each has a `code`, a `level` (`warning` or `info`), a `message` and, where it has them, a
`path` and a `span`.

- `ambiguous_key_match` (diff.json): a key appears more than once on both sides, so which copy
  paired with which may be a guess. a block header's key hint counts across the whole file (see
  order policy).
- `unknown_unparsed_construct` (diff.json): a parse finding, below.
- `diff_unreliable_region` (diff.json and plan.json): in diff.json, a top-level sibling that does
  not pair in order where that leaves an edit (see order policy), or a block nested too deep to
  compare line by line (see plan output). plan.json repeats the second kind where its lines differ.
- `missing_anchor` (plan.json): an edit the plan cannot place in the left-hand file, which gets no
  action.

`netform_ir` records the lines it could not parse with certainty. each such parse finding reaches
diff.json as an `unknown_unparsed_construct` warning that names its code in the message, such as
`left parse uncertainty [orphan-indentation]: …`. the codes:

- `orphan-indentation`: an indented line with no block open above it, kept as a top-level line.
- `mixed-leading-whitespace`: indentation that mixes spaces and tabs, so the nesting may be wrong.
- `unterminated-literal-region`: a banner, or a FortiOS or Junos quoted value spanning lines, that
  never closes; the lines after it are parsed as configuration.
- `unterminated-comment-region`: a Junos `/*` comment that never closes; the lines after it are
  parsed as configuration.

## diff.json

`config-diff --json` writes the `Diff` as json. its top-level keys:

- `normalization_steps`: the steps applied, in order.
- `order_policy`: the `default` policy and the `overrides`, each a `context_prefix` (the PATH as a
  list) and a `policy`.
- `has_changes`: whether there is an edit, or a block nested too deep to compare line by line
  differs. `config-diff` exits 1 when it is true, unless `--no-exit-code` is given.
- `edits`: as above.
- `stats`: the number of `inserts`, `deletes` and `replaces`, and of the lines in them
  (`inserted_lines`, `deleted_lines`, `replaced_old_lines`, `replaced_new_lines`).
- `findings`: as above.

for the override example:

```text
$ config-diff --dialect eos --json --policy-override 1:unordered a.cfg b.cfg | jq -c 'keys_unsorted, .order_policy'
["normalization_steps","order_policy","has_changes","edits","stats","findings"]
{"default":"ordered","overrides":[{"context_prefix":[1],"policy":"unordered"}]}
```

## plan output

`build_plan(&diff)` emits transport-neutral `Plan` actions:

- `replace_block`
- `apply_line_edits_under_context`

plan.json holds `version`, the plan format's version (`"v1"`), then `actions` and `findings`.

every path in a plan names a node of the original left-hand document, the one the plan is applied
to, so `config-diff --plan-json a.cfg b.cfg` plans the changes that turn `a.cfg` into `b.cfg`.
actions do not shift each other's paths: resolve each one against the left-hand document as it was
before the first action. `replace_block.target_path` names the left-side node to replace, its
subtree included, `target_span` is the span of that node's first line, and `intended_lines` are the
lines that replace it, subtree included, as the right-hand file writes them.
`apply_line_edits_under_context.context_path` names the left-side block whose children the line
edits change, a left-side line whose first children they insert, or `[]` for the top level.

line edits carry no paths: a delete picks a child of its context by its text. actions come deepest
first, so everything planned inside a block is applied before its parent's deletes pick among that
block and its siblings. apply them in order.

an action's line edits are its deletes, then its replaces, then its inserts. a block that is
removed or added is planned whole under its parent: its header, then its lines in document order.
an inserted line that cannot be placed under a left-side context is reported as a `missing_anchor`
finding instead of an action. lines nested under 127 or more blocks are compared as a whole, not
line by line, so a change among them has no action: each block whose such lines differ is reported
as a `diff_unreliable_region` finding instead, at its left-side path, or with no path when only the
right-hand document has it.

## schemas

`schemas/` holds a json schema for each file netform writes or reads. they refer to each other by
`$id`, so load them all before validating.

- `diff.schema.json`: diff.json, what `--json` writes.
- `plan.schema.json`: plan.json, what `--plan-json` writes.
- `order-policy.schema.json`: the `order_policy` object in diff.json and in a fixture.
- `normalization-pipeline.schema.json`: the `normalization_steps` list in diff.json and in a
  fixture.
- `fixture.schema.json`: a replay fixture in `fixtures/`.

## cli output

the `config-diff` binary is provided by `netform_cli`.

`config-diff a.cfg b.cfg` prints a unified diff report (`--format markdown` for markdown).
`config-diff --json a.cfg b.cfg` prints `diff.json`.
`config-diff --plan-json a.cfg b.cfg` prints `plan.json`.
`config-diff --dialect auto|generic|eos|fortios|iosxe|iosxr|junos|nxos ...` selects the parser profile (default: auto-detected).
`config-diff --order-policy ordered|unordered|keyed-stable ...` controls line ordering semantics.
`config-diff --policy-override PATH:POLICY ...` overrides it under one node.
`config-diff --ignore-comments --ignore-blank-lines --normalize-whitespace --trim-trailing-whitespace --normalize-leading-whitespace ...` enables normalization steps.
