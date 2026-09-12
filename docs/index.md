# docs index

entrypoints:
- [ir model](model.md)
- [diff model](diff.md)
- [heavy config example](heavy-example.md)
- [dev guide](dev.md)
- top-level readme: `README.md`

how to use this repo:

- parse configs into a lossless `Document` with `netform_ir::parse_generic`
- render with `Document::render()` to preserve exact line text and endings
- parse as a specific vendor with `netform_dialects::find(name)` and the `parse` on its entry
- compare two documents with `netform_diff::diff_documents`
- emit unified, markdown, or json reports using `netform_cli`'s `config-diff` (dialect auto-detected, or forced with `--dialect`)

quick start:

```bash
cargo test --workspace
cargo run -p netform_cli --bin config-diff -- netform_ir/testdata/cisco_like.conf netform_ir/testdata/junos_set_style.conf
cargo run -p netform_cli --bin config-diff -- --dialect junos --json ./intended.conf ./actual.conf
```
