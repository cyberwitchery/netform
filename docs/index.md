# docs index

entrypoints:
- [ir model](model.md)
- [diff model](diff.md)
- [heavy config example](heavy-example.md)
- [dev guide](dev.md)
- top-level readme: `README.md`

how to use this repo:

- parse configs into a lossless `Document` with `netform_ir::parse_generic`
- parse iosxe-oriented text with `netform_dialect_iosxe::parse_iosxe`
- parse junos-oriented text with `netform_dialect_junos::parse_junos`
- render with `Document::render()` to preserve exact line text and endings
- compare two documents with `netform_diff::diff_documents`
- emit markdown or json using `config-diff` (`--dialect generic|iosxe|junos`)

quick start:

```bash
cargo test --workspace
cargo run -p netform_diff --bin config-diff -- netform_ir/testdata/cisco_like.conf netform_ir/testdata/junos_set_style.conf
cargo run -p netform_diff --bin config-diff -- --dialect junos --json ./intended.conf ./actual.conf
```
