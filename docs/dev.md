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

## docs build

```bash
RUSTDOCFLAGS="--cfg docsrs" cargo doc --workspace --all-features --no-deps
```

open locally:

- `target/doc/netform_ir/index.html`
- `target/doc/netform_diff/index.html`
- `target/doc/netform_dialect_iosxe/index.html`
- `target/doc/netform_dialect_junos/index.html`

## ci

- ci workflow: `.github/workflows/ci.yml`
- release workflow: `.github/workflows/release.yml`
