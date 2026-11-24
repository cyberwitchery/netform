# developer docs

this section covers contributor workflows and local validation steps.

## build and test

```bash
cargo build
cargo test --workspace --all-targets
```

## lint and format

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## release readiness

one-command local gate:

```bash
./scripts/run_release_readiness.sh
```

optional extras:

```bash
./scripts/run_release_readiness.sh --with-miri
./scripts/run_release_readiness.sh --with-publish-dry-run
```

## miri

run the local helper:

```bash
./scripts/run_miri.sh
```

optional full pass:

```bash
MIRIFLAGS='-Zmiri-disable-isolation' ./scripts/run_miri.sh --full
```

note:
- `--full` excludes subprocess-based CLI smoke tests because Miri on macOS does not support `std::process` spawning.
- tune property-test runtime under Miri with `MIRI_PROPTEST_CASES` (default `8`), for example:
  `MIRI_PROPTEST_CASES=4 MIRIFLAGS='-Zmiri-disable-isolation' ./scripts/run_miri.sh --full`

## docs build

```bash
cargo doc --workspace --all-features --no-deps
```

open locally:

- `target/doc/netform_ir/index.html`
- `target/doc/netform_dialects/index.html`
- `target/doc/netform_diff/index.html`
- `target/doc/netform_cli/index.html`

## ci

`.github/workflows/ci.yml` runs on pushes to `main` and on pull requests, unless every changed file
is markdown. its `success` job passes only when every other job passed:

- `fmt`: `cargo fmt --all --check`
- `clippy`: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `test`: `cargo test --workspace --all-targets`. this includes `netform_diff/tests/fixture_replay.rs`,
  which diffs each fixture in `fixtures/` and compares `has_changes`, the full edits and the finding
  codes with the fixture's `expected`, see [fixtures](#fixtures)
- `doc`: `cargo doc --workspace --no-deps` with `RUSTDOCFLAGS=-D warnings`
- `deny`: `cargo deny check` with `deny.toml`
- `coverage`: `cargo llvm-cov --workspace --all-targets --summary-only --fail-under-lines 80`, which
  fails below 80% line coverage
- `unsafe-budget`: checks the `unsafe` counts of the workspace and its dependencies against
  `unsafe-budget.lock`
- `sbom`: generates each crate's sbom for the head and the last `v*` tag, writes their diff to the job
  summary and warns about components missing from `sbom/`. it never fails on content

`.github/workflows/release.yml` runs when a `v*` tag is pushed:

- `sbom`: packages each crate, generates its sbom and fails on components missing from `sbom/`
- `attest`, after `sbom`: attests each packaged crate's provenance and sbom
- `publish`, after `sbom`: runs the [docs build](#docs-build) and `cargo test --workspace --all-targets`,
  attaches the sboms to the tag's github release, checks that the working tree is clean, then runs
  `cargo publish --no-verify` for `netform_ir`, `netform_dialects`, `netform_diff` and `netform_cli`,
  in that order. a version already on crates.io counts as published, so running the release again
  finishes a partial one

release.yml runs no fmt, clippy, deny, coverage or unsafe-budget check.

## fixtures

each file in `fixtures/` is one replay case in the format `schemas/fixture.schema.json` describes.
`fixtures/basic_replace.json`:

```json
{
  "name": "basic_replace",
  "intended": "interface Ethernet1\n  description intended\n",
  "actual": "interface Ethernet1\n  description actual\n",
  "normalization_steps": [],
  "order_policy": {
    "default": "ordered",
    "overrides": []
  },
  "expected": {
    "has_changes": true,
    "edits": [
      {
        "type": "Replace",
        "old_at_key": 6148859732172756841,
        "new_at_key": 12309690679627054692,
        "left_anchor": {"path": [0, 0], "span": {"line": 2, "start_byte": 20, "end_byte": 42}},
        "right_anchor": {"path": [0, 0], "span": {"line": 2, "start_byte": 20, "end_byte": 40}},
        "old_lines": [
          {"content_key": 15578098766994011527, "occurrence_key": 6148859732172756841, "text": "  description intended", "path": [0, 0], "span": {"line": 2, "start_byte": 20, "end_byte": 42}}
        ],
        "new_lines": [
          {"content_key": 14849657242699640321, "occurrence_key": 12309690679627054692, "text": "  description actual", "path": [0, 0], "span": {"line": 2, "start_byte": 20, "end_byte": 40}}
        ]
      }
    ],
    "finding_codes": []
  }
}
```

- `name`: names the case in test failures. the file is `fixtures/<name>.json`
- `dialect`: the vendor that parses both configs: `generic` (the default), `eos`, `fortios`, `iosxe`,
  `iosxr`, `junos` or `nxos`
- `intended`, `actual`: the two configs, as `config-diff`'s first and second file
- `normalization_steps`: any of `ignore_comments`, `ignore_blank_lines`, `trim_trailing_whitespace`,
  `normalize_leading_whitespace` and `collapse_internal_whitespace`
- `order_policy`: the `default` policy (`ordered`, `unordered` or `keyed-stable`) and the `overrides`,
  each a `context_prefix` index path and a `policy`, as `--order-policy` and `--policy-override` set them
- `expected`: what the diff must give

`fixture_replay.rs` diffs each fixture and compares the result with `expected`:

- `has_changes` with the diff's `has_changes`
- `edits` with the diff's edits as `config-diff --json` writes them, compared as parsed json so layout
  does not matter: each edit's type, its `at_key` (or `old_at_key` and `new_at_key`), its anchors, and
  each line's keys, text, path and span, in order. a change to how keys are hashed or lines are paired
  fails the replay
- `finding_codes` with the codes of the diff's findings, in order. their levels, messages, paths and
  spans are not compared

`schema_validation.rs` also checks each fixture against `schemas/fixture.schema.json`, and the diff and
plan it gives with each order policy as the default against `diff.schema.json` and `plan.schema.json`.

there is no regeneration command. to add a fixture, write it with the `has_changes` you expect and
`"edits": []`, run `cargo test -p netform_diff --test fixture_replay`, check the edits the failure
prints after `<name>: edits differ, got:` and paste them into `expected.edits`. a wrong
`finding_codes` fails next and prints the codes the diff gave as `left`.
