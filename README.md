# netform

vendor-agnostic, lossless config parsing and diffing for network configs.

## structure

- `netform_ir`: core config intermediate representation (ir), parser, and lossless renderer.
- `netform_diff`: normalization, diff engine, report formatting, and plan/report primitives.
- `netform_cli`: `config-diff` and replay binaries.
- `netform_dialect_eos`: eos profile for comment/token handling and dialect-aware parsing.
- `netform_dialect_iosxe`: iosxe profile for comment/token handling and dialect-aware parsing.
- `netform_dialect_junos`: junos profile for comment/token handling and dialect-aware parsing.
- `netform_dialect_nxos`: nxos profile for comment/token handling and dialect-aware parsing.
- `netform_dialect_fortios`: fortios profile for comment/token handling and dialect-aware parsing.

## features

- lossless round-trip: parse -> render preserves original text
- indentation-based structural grouping with conservative fallback
- stable node ids and path addressing for diff output
- configurable normalization (comments, blank lines, whitespace)
- deterministic line-based edits with spans and stats
- unified or markdown reports plus machine-readable `diff.json` / `plan.json`

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
netform_ir = "0.7.0"
netform_diff = "0.7.0"
netform_dialect_eos = "0.7.0"
netform_dialect_iosxe = "0.7.0"
netform_dialect_junos = "0.7.0"
netform_dialect_nxos = "0.7.0"
netform_dialect_fortios = "0.7.0"
```

install the cli binary so you can run `config-diff` directly:

```bash
# from this repo checkout
cargo install --path netform_cli

# or from crates.io
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

let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
assert_eq!(diff.stats.replaces, 1);
```

## cli

`config-diff` compares two config files and prints a drift report.

usage:

```text
config-diff [OPTIONS] <FILE_A> <FILE_B>
```

options:

- `--dialect <auto|generic|eos|fortios|iosxe|junos|nxos>`: parser profile (default: `auto`, detected from content; falls back to `generic` and warns when the two files disagree)
- `--format <unified|markdown>`: human-readable report format (default: `unified`)
- `--context-lines <n>`: lines shown per side of each edit before truncating (default: 10)
- `--order-policy <ordered|unordered|keyed-stable>`: sibling ordering semantics (default: `ordered`)
- `--policy-override <PATH:POLICY>`: per-context order-policy override (repeatable)
- `--ignore-comments`: drop comment lines from comparison
- `--ignore-blank-lines`: drop blank lines from comparison
- `--normalize-whitespace`: collapse internal whitespace in comparison view
- `--trim-trailing-whitespace`: drop trailing whitespace in comparison view
- `--normalize-leading-whitespace`: normalize indentation in comparison view
- `--json`: print machine-readable `Diff` json instead of a report
- `--plan-json`: print machine-readable `Plan` json instead of a report
- `--color` / `--no-color`: force or disable colored output
- `--no-exit-code`: exit 0 even when configs differ (by default, config-diff exits 1 on drift, like `diff(1)`; exit 2 means an I/O or serialization error)

examples:

```bash
cargo run -p netform_cli --bin config-diff -- ./before.cfg ./after.cfg
cargo run -p netform_cli --bin config-diff -- --dialect eos ./intended.conf ./actual.conf
cargo run -p netform_cli --bin config-diff -- --dialect iosxe ./intended.conf ./actual.conf
cargo run -p netform_cli --bin config-diff -- --dialect junos ./intended.conf ./actual.conf
cargo run -p netform_cli --bin config-diff -- --dialect nxos ./intended.conf ./actual.conf
cargo run -p netform_cli --bin config-diff -- --dialect fortios ./intended.conf ./actual.conf
cargo run -p netform_cli --bin config-diff -- --order-policy keyed-stable ./intended.conf ./actual.conf
cargo run -p netform_cli --bin config-diff -- --json ./before.cfg ./after.cfg
cargo run -p netform_cli --bin config-diff -- --plan-json ./before.cfg ./after.cfg
# always exit 0, even when configs have drifted
cargo run -p netform_cli --bin config-diff -- --no-exit-code ./intended.conf ./actual.conf
```

## release

releases are tag-driven (`v*`) via github actions and publish workspace crates to crates.io.

<hr/>

have fun!
