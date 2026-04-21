# changelog

## [unreleased]

- `format_markdown_report` now shows actual config line text under each edit (with `+`/`-` diff markers), so `config-diff` default output reveals what changed — not just how many lines and at which key
- added `netform_ir::IosLikeDialect` — parameterized dialect struct shared by all IOS-like dialect crates; `EosDialect` and `IosxeDialect` are now type aliases, and adding a new IOS-like dialect requires only `IosLikeDialect::new("name")`
- replaced O(n²) linear scans in diff engine with hash-based lookups: `HashMap` for context-path grouping in plan builder, `HashSet` for key-union dedup in multiset diff — improves performance on large configs
- added `--trim-trailing-whitespace` and `--normalize-leading-whitespace` CLI flags — two implemented normalization steps that were previously unreachable from the command line
- implemented `Display` for `FindingLevel` so text report output uses lowercase `warning`/`info` consistent with JSON output
- expanded `junos_key_hint` to cover firewall, security, snmp, vlans, chassis, class-of-service, forwarding-options, applications, and groups stanzas, plus set-style equivalents — reduces spurious ambiguous-key findings for Junos configs
- expanded `ios_like_key_hint` to cover class-map, policy-map, ip community-list, numbered access-lists, crypto constructs (isakmp, map, ikev2, ipsec), and spanning-tree vlan — reduces spurious ambiguous-key findings for configs with multiple instances of these blocks
- added `netform_ir::classify_ios_like_trivia` and `netform_ir::parse_ios_like_parts` — shared trivia classification and line tokenization for IOS-like dialects, replacing identical copies in the EOS and IOS XE dialect crates
- made `netform_ir::count_indent` public so `netform_diff` can reuse it instead of maintaining a duplicate `count_indent_columns`
- added `netform_ir::ios_like_key_hint` — shared key-hint derivation for IOS-like dialects, replacing identical copies in the EOS and IOS XE dialect crates
- `config-diff` now uses differentiated exit codes following `diff(1)` convention: 0 = no differences, 1 = differences found, 2 = error (I/O, serialization); `--no-exit-code` suppresses exit 1 but not exit 2

## [0.3.0] - 2026-04-16

- added `netform_ir::tokenize` — shared quote-aware tokenizer parameterized by punctuation characters, replacing identical state machines in the EOS, Junos, and IOS XE dialect crates
- fixed `CollapseInternalWhitespace` stripping leading indentation — hierarchical configs no longer produce spurious matches between lines at different nesting levels
- `config-diff` now exits 1 when configs differ by default (like `diff(1)`); use `--no-exit-code` to suppress

## [0.2.0] - 2026-02-17

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
