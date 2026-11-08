#!/usr/bin/env bash
set -euo pipefail

WITH_MIRI=0
WITH_PUBLISH_DRY_RUN=0

usage() {
  cat <<'EOF'
Usage: scripts/run_release_readiness.sh [--with-miri] [--with-publish-dry-run] [--help]

Runs release-readiness checks for netform.

Options:
- --with-miri: run local Miri-safe subset via scripts/run_miri.sh
- --with-publish-dry-run: run cargo publish --dry-run for all workspace crates
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --with-miri)
      WITH_MIRI=1
      shift
      ;;
    --with-publish-dry-run)
      WITH_PUBLISH_DRY_RUN=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

echo "[1/8] format"
cargo fmt --all --check

echo "[2/8] clippy"
cargo clippy --workspace --all-targets --all-features -- -D warnings

echo "[3/8] tests"
cargo test --workspace --all-targets

echo "[4/8] doctests"
cargo test --workspace --doc

echo "[5/8] docs build"
RUSTDOCFLAGS="--cfg docsrs" cargo doc --workspace --all-features --no-deps

echo "[6/8] fixture replay"
cargo run -q -p netform_cli --bin netform-replay-fixtures

echo "[7/8] coverage gate"
cargo llvm-cov --workspace --all-targets --summary-only --fail-under-lines 80

if [[ "${WITH_MIRI}" -eq 1 ]]; then
  echo "[8/8] miri subset"
  ./scripts/run_miri.sh
fi

if [[ "${WITH_PUBLISH_DRY_RUN}" -eq 1 ]]; then
  echo "[extra] publish dry-run (requires network access)"
  cargo publish -p netform_ir --dry-run
  cargo publish -p netform_dialects --dry-run
  cargo publish -p netform_diff --dry-run
  cargo publish -p netform_cli --dry-run
fi

echo "release-readiness checks passed"
