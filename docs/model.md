# ir model

`netform_ir` models a config as a lossless tree backed by an arena.

## core types

- `Document`: metadata + root node ids + node arena
- `Node`: `Line` or `Block`
- `LineNode`:
  - `raw`: original line text without trailing newline
  - `line_ending`: `""`, `"\n"`, or `"\r\n"`
  - `span`: line number and byte offsets in source text
  - `parsed`: optional `head` + `args` tokenization
  - `key_hint`: optional dialect-provided identity hint for keyed diffing
  - `trivia`: `Blank`, `Comment`, `Content`, `Literal`, or `Unknown`
- `BlockNode`:
  - `header`: a `LineNode`
  - `children`: `NodeId` list
  - `footer`: optional closing `LineNode` for delimiter-terminated dialects (FortiOS `end`/`next`, Junos `}`/`};`)
  - `kind_label`: optional label (reserved for future semantic tagging)

## parser behavior (v1)

- indentation is the only structural signal
- if a content line is followed by a more-indented content line, it opens a block
- non-blank dedent closes blocks
- a dialect may mark a line as a block terminator (via `Dialect::block_terminator`); a terminator that closes a block is attached to that block as its `footer` instead of being kept as a sibling
- a dialect may open a multi-line literal region (via `Dialect::literal_region`) for free text such as IOS-family banners and FortiOS or Junos multi-line quoted values; its lines are `Literal`, are never tokenized or key-hinted, and neither open nor close blocks
- unknown patterns are preserved as regular `Line` nodes
- no line is dropped

## round-trip guarantee

renderer emits line `raw + line_ending` in original traversal order.
for valid `Document` output from `parse_generic`, `parse(render(x)) == x` at text level.
