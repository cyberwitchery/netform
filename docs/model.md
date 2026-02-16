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
  - `trivia`: `Blank`, `Comment`, `Content`, or `Unknown`
- `BlockNode`:
  - `header`: a `LineNode`
  - `children`: `NodeId` list
  - `footer`: optional `LineNode` (reserved for future dialects)
  - `kind_label`: optional label (reserved for future semantic tagging)

## parser behavior (v1)

- indentation is the only structural signal
- if a content line is followed by a more-indented content line, it opens a block
- non-blank dedent closes blocks
- unknown patterns are preserved as regular `Line` nodes
- no line is dropped

## round-trip guarantee

renderer emits line `raw + line_ending` in original traversal order.
for valid `Document` output from `parse_generic`, `parse(render(x)) == x` at text level.

