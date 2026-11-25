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
  - `trivia`: `Blank`, `Comment`, `Content`, or `Literal`
- `BlockNode`:
  - `header`: a `LineNode`
  - `children`: `NodeId` list
  - `footer`: optional closing `LineNode` for delimiter-terminated dialects (FortiOS `end`/`next`, Junos `}`/`};`, IOS XR `end-policy`/`end-set`)

## parser behavior

- indentation is the structural signal, except for a dialect that also nests by keyword (FortiOS, below)
- a content line opens a block when the next content or comment line is more indented
- a content or comment line that dedents closes blocks
- a dialect may mark a line as a block terminator (via `Dialect::block_terminator`); a terminator that closes a block is attached to that block as its `footer` instead of being kept as a sibling
- a dialect may pair a content line with the terminator directly after it at the same indent (via `Dialect::empty_block`), such as Junos `ge-0/0/0 {` and `}`; the pair parses as an empty block with that terminator as its `footer`
- a dialect may nest by keyword (via `Dialect::block_opener`): openers and terminators match like brackets, a terminator matching the innermost open opener when they share an indent and `Dialect::empty_block` pairs them. the block holds every line between the two whatever its indent, a dedent inside it does not close it, and the terminator is its `footer`. any other terminator, or an opener with 64 others already open, leaves the openers still open unmatched, and unmatched openers and terminators are placed by indentation. FortiOS nests `config`/`end` and `edit`/`next` this way, since a multi-VDOM configuration writes `config vdom`, `edit <vdom>` and `config global` and the tables directly under them at column 0
- a dialect may open a multi-line literal region (via `Dialect::literal_region`) for free text such as IOS-family banners and FortiOS or Junos multi-line quoted values; its lines are `Literal`, are never tokenized or key-hinted, and neither open nor close blocks
- a dialect may likewise open a multi-line comment region (via `Dialect::comment_region`) for constructs such as Junos `/* ... */`; its lines are `Comment`, so `--ignore-comments` still drops them, but like a literal region's they neither open nor close blocks, and a comment region wins over a literal region opened on the same line
- a dialect may read a double-quoted `"…"` as one value (via `Dialect::quotes_delimit_values`), as FortiOS and Junos do; the document's metadata records this, and the diff's whitespace normalization leaves the inside of such a value as written
- a dialect may name leading words that mark a statement without changing which statement it is (via `Dialect::annotations`), such as Junos `inactive:` and `protect:`; the document's metadata records them, and the diff keys a marked statement as the same statement unmarked
- unknown patterns are preserved as regular `Line` nodes
- no line is dropped

## round-trip guarantee

renderer emits line `raw + line_ending` in original traversal order, so a parsed document renders
back to its input byte for byte: `render(parse(text)) == text`. the tests check this for
`parse_generic` on random input and for each vendor's parser on sample configurations.
