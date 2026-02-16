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
cargo clippy --workspace --all-targets --all-features
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
- `target/doc/netform_diff/index.html`
- `target/doc/netform_cli/index.html`
- `target/doc/netform_dialect_eos/index.html`
- `target/doc/netform_dialect_iosxe/index.html`
- `target/doc/netform_dialect_junos/index.html`

## ci

- ci workflow: `.github/workflows/ci.yml`
- release workflow: `.github/workflows/release.yml`
