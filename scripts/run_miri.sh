#!/usr/bin/env bash
set -euo pipefail

TOOLCHAIN="${MIRI_TOOLCHAIN:-nightly}"
MIRI_PROPTEST_CASES="${MIRI_PROPTEST_CASES:-8}"

usage() {
  cat <<'EOF'
Usage: scripts/run_miri.sh [--full] [--help]

Runs Miri checks for netform.

Default mode:
- runs a Miri-safe subset that works with isolation enabled

--full:
- runs full Miri-capable suite (all crates/targets except subprocess CLI smoke tests)
- commonly requires: MIRIFLAGS='-Zmiri-disable-isolation'

Environment:
- MIRI_TOOLCHAIN: toolchain to use (default: nightly)
- MIRI_PROPTEST_CASES: proptest case count for Miri property tests (default: 8)
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi

if ! cargo +"${TOOLCHAIN}" miri --version >/dev/null 2>&1; then
  echo "cargo-miri is not available for toolchain '${TOOLCHAIN}'." >&2
  echo "Install with: rustup component add --toolchain ${TOOLCHAIN} miri" >&2
  exit 1
fi

echo "Using toolchain: ${TOOLCHAIN}"

if [[ "${1:-}" == "--full" ]]; then
  echo "Running full Miri-capable suite..."
  # On macOS, Miri does not support std::process spawning (posix_spawnattr_init),
  # so we intentionally exclude subprocess-based cli_smoke integration tests.
  cargo +"${TOOLCHAIN}" miri test -p netform_ir --all-targets
  cargo +"${TOOLCHAIN}" miri test -p netform_dialect_iosxe --all-targets
  cargo +"${TOOLCHAIN}" miri test -p netform_dialect_junos --all-targets
  cargo +"${TOOLCHAIN}" miri test -p netform_diff --lib --bins
  cargo +"${TOOLCHAIN}" miri test -p netform_diff --test contract_shape
  cargo +"${TOOLCHAIN}" miri test -p netform_diff --test determinism_corpus
  cargo +"${TOOLCHAIN}" miri test -p netform_diff --test plan_output
  PROPTEST_CASES="${MIRI_PROPTEST_CASES}" cargo +"${TOOLCHAIN}" miri test -p netform_diff --test properties
  cargo +"${TOOLCHAIN}" miri test -p netform_diff --test report_output
  exit 0
fi

echo "Running Miri-safe subset..."
# Keep this subset isolation-safe: no subprocess spawning, no testdata directory scans.
cargo +"${TOOLCHAIN}" miri test -p netform_diff --lib
cargo +"${TOOLCHAIN}" miri test -p netform_ir --test round_trip --test parser_structure
cargo +"${TOOLCHAIN}" miri test -p netform_dialect_iosxe --all-targets
cargo +"${TOOLCHAIN}" miri test -p netform_dialect_junos --all-targets
