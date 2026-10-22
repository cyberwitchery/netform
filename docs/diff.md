# diff model

`netform_diff` computes deterministic edits from two `netform_ir::Document` values.

top-level drift is exposed as `Diff.has_changes`.

## normalization

`NormalizeOptions` uses an explicit ordered step pipeline:

- `ignore_comments`
- `ignore_blank_lines`
- `trim_trailing_whitespace`
- `normalize_leading_whitespace`
- `collapse_internal_whitespace`

applied steps are recorded in `Diff.normalization_steps`.

## order policy

ordering is explicit and reproducible through `OrderPolicyConfig`:

- `ordered`
- `unordered`
- `keyed-stable`

resolved policy config is emitted in `Diff.order_policy`.

## comparison view

each comparable line carries:

- normalized text (for matching)
- original text (for reporting)
- `content_key` (semantic hash key)
- `occurrence_key` (stable disambiguation hash)
- `Path` (node path)
- `Span` (line + byte offsets)
- trivia classification
- optional dialect-provided `key_hint` used for keyed-stable matching when available

## edits

v1 emits grouped edit-script operations:

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

## findings

`Diff.findings` always carries explicit uncertainty/warning signals with stable codes:

- `unknown_unparsed_construct`
- `ambiguous_key_match`
- `diff_unreliable_region`

## plan output

`build_plan(&diff)` emits transport-neutral `Plan` actions:

- `replace_block`
- `apply_line_edits_under_context`

every path in a plan names a node of the original left-hand document, the one the plan is applied
to, so `config-diff --plan-json a.cfg b.cfg` plans the changes that turn `a.cfg` into `b.cfg`.
actions do not shift each other's paths: resolve each one against the left-hand document as it was
before the first action. `replace_block.target_path` names the left-side node to replace, its
subtree included. `apply_line_edits_under_context.context_path` names the left-side block whose
children the line edits change, a left-side line whose first children they insert, or `[]` for the
top level.

line edits carry no paths: a delete picks a child of its context by its text. actions come deepest
first, so everything planned inside a block is applied before its parent's deletes pick among that
block and its siblings. apply them in order.

an action's line edits are its deletes, then its replaces, then its inserts. a block that is
removed or added is planned whole under its parent: its header, then its lines in document order.
an inserted line that cannot be placed under a left-side context is reported as a `missing_anchor`
finding instead of an action.

## cli output

the `config-diff` binary is provided by `netform_cli`.

`config-diff a.cfg b.cfg` prints a unified diff report (`--format markdown` for markdown).
`config-diff --json a.cfg b.cfg` prints `diff.json`.
`config-diff --plan-json a.cfg b.cfg` prints `plan.json`.
`config-diff --dialect auto|generic|eos|fortios|iosxe|iosxr|junos|nxos ...` selects the parser profile (default: auto-detected).
`config-diff --order-policy ordered|unordered|keyed-stable ...` controls line ordering semantics.
`config-diff --ignore-comments --ignore-blank-lines --normalize-whitespace ...` enables normalization steps.
