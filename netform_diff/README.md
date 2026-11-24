# netform_diff

the diff engine for netform documents. it compares two parsed configs under explicit normalization
and ordering policies, and emits deterministic line edits with spans, stats and findings. reports
come as unified or markdown text; `Diff` and `Plan` serialize to the `diff.json` and `plan.json`
that `config-diff` prints.

## usage

```rust
use netform_diff::{DEFAULT_CONTEXT_LINES, NormalizeOptions, diff_documents, format_markdown_report};
use netform_ir::parse_generic;

let before = parse_generic("interface Ethernet1\n  description old\n");
let after = parse_generic("interface Ethernet1\n  description new\n");

let diff = diff_documents(&before, &after, NormalizeOptions::default()).unwrap();
assert_eq!(diff.stats.replaces, 1);

let report = format_markdown_report(&diff, "before.cfg", "after.cfg", DEFAULT_CONTEXT_LINES);
assert!(report.contains("- Replaces: 1 (1 -> 1 lines)"));
```

to diff a vendor's config, parse it with
[netform_dialects](https://crates.io/crates/netform_dialects) instead of `parse_generic`.

## docs

- [diff model](https://github.com/cyberwitchery/netform/blob/main/docs/diff.md)
- [heavy config example](https://github.com/cyberwitchery/netform/blob/main/docs/heavy-example.md)
- [project readme](https://github.com/cyberwitchery/netform#readme)
