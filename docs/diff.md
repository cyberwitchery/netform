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

## cli output

the `config-diff` binary is provided by `netform_cli`.

`config-diff a.cfg b.cfg` prints markdown report.
`config-diff --json a.cfg b.cfg` prints `diff.json`.
`config-diff --plan-json a.cfg b.cfg` prints `plan.json`.
`config-diff --dialect generic|eos|iosxe|junos ...` selects parser profile.
`config-diff --order-policy ordered|unordered|keyed-stable ...` controls line ordering semantics.
`config-diff --ignore-comments --ignore-blank-lines --normalize-whitespace ...` enables normalization steps.
