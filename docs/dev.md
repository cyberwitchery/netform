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
RUSTDOCFLAGS="--cfg docsrs" cargo doc --workspace --all-features --no-deps
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
  codes with the fixture's `expected`
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
