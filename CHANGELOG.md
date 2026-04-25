# changelog

## [unreleased]

- added `finding_code` module with typed constants (`MISSING_ANCHOR`, `AMBIGUOUS_KEY_MATCH`, `UNKNOWN_UNPARSED_CONSTRUCT`, `DIFF_UNRELIABLE_REGION`) — diagnostic codes are now defined in one place instead of scattered string literals
- `PlanFinding` now carries optional `level`, `path`, and `span` fields (matching `Finding`), serialized only when present — plan consumers can now pinpoint which config location triggered a diagnostic
- expanded `ios_like_key_hint` with IPv6 constructs: `ipv6 access-list`, `ipv6 prefix-list`, and `ipv6 route` (with VRF support) — IPv6 config diffs now correctly match these stanzas instead of flagging them as ambiguous
- expanded `junos_key_hint` to cover `system` (with sub-stanza hints for host-name, services, login, ntp, syslog) and `routing-options` stanzas, both hierarchical and set-style — reduces spurious ambiguous-key findings for Junos configs using these common stanzas
- narrowed the `*` comment prefix in `classify_junos_trivia` to `* ` (asterisk-space) — bare `*` without a trailing space is no longer misclassified as a comment, preventing false-positive comment stripping when `IgnoreComments` is active
- `config-diff` default output is now a colored unified diff (`---`/`+++` headers, red `-` deletions, green `+` insertions, cyan `@@` hunk headers) instead of markdown — colors are auto-detected from TTY and can be forced with `--color`/`--no-color`; JSON and plan-JSON modes are unchanged
- added `--context-lines N` CLI flag — controls how many lines are shown per side of each edit in the default markdown report before truncating; defaults to 10 (the previous hardcoded value)
- `line_diff_multiset` now emits one edit per differing key bucket instead of collapsing all changes into a single monolithic Replace — multiset diffs (keyed-stable, unordered) with multiple changed fields produce per-field edits, so the plan builder takes the fine-grained `ApplyLineEditsUnderContext` path rather than the coarse `ReplaceBlock` arm
- expanded `fortios_key_hint` with `set:<field>` and `unset:<field>` subkey hints — FortiOS diffs now treat value changes on the same field as modifications rather than spurious delete + add pairs; the diff engine uses leaf-line hints for stable content-key hashing without exposing them as extracted keys
- added `netform_dialect_fortios` crate — Fortinet FortiOS dialect with `config`/`edit` key hints, `#` comment classification, and quoted-string tokenization, wired into the CLI as `--dialect fortios`
- expanded `ios_like_key_hint` with NX-OS-specific stanza types: `feature`, `vpc domain`, `role name`, `monitor session`, `ntp server`/`peer`, and `system` — NX-OS config diffs now correctly match these constructs instead of flagging them as ambiguous
- added `netform_dialect_nxos` crate — Cisco NX-OS dialect using `IosLikeDialect`, wired into the CLI as `--dialect nxos`
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
