# netform

vendor-agnostic, lossless config parsing and diffing for network configs.

## structure

- `netform_ir`: core config intermediate representation (ir), parser, and lossless renderer.
- `netform_diff`: normalization, diff engine, report formatting, and plan/report primitives.
- `netform_cli`: `config-diff` and replay binaries.
- `netform_dialect_eos`: eos profile for comment/token handling and dialect-aware parsing.
- `netform_dialect_iosxe`: iosxe profile for comment/token handling and dialect-aware parsing.
- `netform_dialect_junos`: junos profile for comment/token handling and dialect-aware parsing.

## features

- lossless round-trip: parse -> render preserves original text
- indentation-based structural grouping with conservative fallback
- stable node ids and path addressing for diff output
- configurable normalization (comments, blank lines, whitespace)
- deterministic line-based edits with spans and stats
- markdown report output plus `diff.json` / `plan.json`

## docs

- [docs index](docs/index.md)
- [ir model](docs/model.md)
- [diff model](docs/diff.md)
- [heavy config example](docs/heavy-example.md)
- [dev guide](docs/dev.md)

## install

add to `Cargo.toml`:

```toml
[dependencies]
netform_ir = "0.2.0"
netform_diff = "0.2.0"
netform_dialect_eos = "0.2.0"
netform_dialect_iosxe = "0.2.0"
netform_dialect_junos = "0.2.0"
```

install the cli binary so you can run `config-diff` directly:

```bash
# from this repo checkout
cargo install --path netform_cli

# or from crates.io (after publish)
cargo install netform_cli
```

## quick start

parse and round-trip:

```rust
use netform_dialect_junos::parse_junos;

let input = "interfaces {\n    ge-0/0/0 {\n        disable;\n    }\n}\n";
let doc = parse_junos(input);
assert_eq!(doc.render(), input);
```

diff two configs:

```rust
use netform_diff::{diff_documents, NormalizeOptions};
use netform_ir::parse_generic;

let a = parse_generic("interface Ethernet1\n  description old\n");
let b = parse_generic("interface Ethernet1\n  description new\n");

let diff = diff_documents(&a, &b, NormalizeOptions::default());
assert_eq!(diff.stats.replaces, 1);
```

## cli

`config-diff` compares two config files and prints a drift report.

usage:

```text
config-diff [OPTIONS] <FILE_A> <FILE_B>
```

options:

- `--dialect <generic|eos|iosxe|junos>`: parser profile to apply (default: `generic`)
- `--order-policy <ordered|unordered|keyed-stable>`: sibling ordering semantics (default: `ordered`)
- `--ignore-comments`: drop comment lines from comparison
- `--ignore-blank-lines`: drop blank lines from comparison
- `--normalize-whitespace`: collapse internal whitespace in comparison view
- `--json`: print machine-readable `Diff` json instead of markdown
- `--plan-json`: print machine-readable `Plan` json instead of markdown

examples:

```bash
cargo run -p netform_cli --bin config-diff -- ./before.cfg ./after.cfg
cargo run -p netform_cli --bin config-diff -- --dialect eos ./intended.conf ./actual.conf
cargo run -p netform_cli --bin config-diff -- --dialect iosxe ./intended.conf ./actual.conf
cargo run -p netform_cli --bin config-diff -- --dialect junos ./intended.conf ./actual.conf
cargo run -p netform_cli --bin config-diff -- --order-policy keyed-stable ./intended.conf ./actual.conf
cargo run -p netform_cli --bin config-diff -- --json ./before.cfg ./after.cfg
cargo run -p netform_cli --bin config-diff -- --plan-json ./before.cfg ./after.cfg
```

## release

releases are tag-driven (`v*`) via github actions and publish workspace crates to crates.io.

<hr/>

have fun!
