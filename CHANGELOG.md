# changelog

## [unreleased]

## [0.2.0] - 2026-02-16

- added `netform_cli` crate for binaries (`config-diff`, `netform-replay-fixtures`)
- split `netform_diff` into focused modules (`model`, `normalize`, `flatten`, `engine`, `findings`, `report`, `plan`, `util`)
- replaced quadratic lcs matrix alignment with deterministic myers ses edit-script generation
- moved cli smoke coverage to `netform_cli/tests/cli_smoke.rs`
- moved key hint extraction ownership into dialect crates (`netform_dialect_eos`, `netform_dialect_iosxe`, `netform_dialect_junos`)
- expanded fixture corpus with heavier iosxe/junos/eos scenarios and replay coverage
- updated docs/readme to reflect crate boundaries and cli install/run paths
- hardened release-readiness script for current workspace crates and replay command

## [0.1.0] - 2026-02-16

- initial version
