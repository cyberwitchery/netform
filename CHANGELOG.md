# changelog

## [unreleased]

- replaced `IosLikeDialect` re-export in `netform_dialect_iosxe` with a dedicated `IosxeDialect` struct implementing the `Dialect` trait directly — IOS XE configs now get IOS XE-specific key-hint derivation with interface type normalization for 15 interface types (`GigabitEthernet0/0/0` → `interface:gigabitethernet:0/0/0`, `TenGigabitEthernet1/0/1` → `interface:tengigabitethernet:1/0/1`, plus TwentyFiveGigE, FortyGigabitEthernet, HundredGigE, TwoGigabitEthernet, FiveGigabitEthernet, AppGigabitEthernet, FastEthernet, Port-channel, Loopback, Tunnel, Serial, Vlan, BDI), IOS XE-style `vrf definition` handling, `router ospf`/`router eigrp` with process/AS identifiers, `ip access-list` bare form, and IOS XE-specific constructs: `crypto pki`, `redundancy`, `parameter-map type`, `track`, `zone security`, `zone-pair security`
- extracted dialect auto-detection (`detect_dialect`, `auto_parse`) from the CLI binary into `netform_ir::detect` — library consumers can now use score-based dialect detection without depending on `netform_cli`; `detect_dialect` returns `DialectHint` instead of the CLI-internal `CliDialect` enum, and `auto_parse` provides a one-call detect-and-parse convenience function
- extracted `common_key_hint` in `netform_ir` to deduplicate ~12 identical match arms (`vlan`, `route-map`, `class-map`, `policy-map`, `ipv6`, `access-list`, `crypto`, `spanning-tree`, `line`, `monitor`, `ntp`) that were copied across `ios_like_key_hint`, `eos_key_hint`, and `nxos_key_hint` — each dialect-specific function now handles only its own constructs and falls back to `common_key_hint` for the shared ones
- **breaking:** removed NX-OS-specific arms (`feature`, `vpc`, `role`, `system`) from `ios_like_key_hint` — these now live only in `nxos_key_hint` where they belong; callers using `IosLikeDialect` (i.e. IOS XE) will no longer produce key hints for NX-OS-only constructs
- fixed dialect auto-detection tie-breaking: when two dialects score equally, `detect_dialect` now explicitly falls back to `Generic` rather than relying on array-position tie-breaking that happened to be caught by the margin check — makes the behavior immune to future changes in the margin threshold or candidate ordering
- replaced `IosLikeDialect` re-export in `netform_dialect_eos` with a dedicated `EosDialect` struct implementing the `Dialect` trait directly — EOS configs now get EOS-specific key-hint derivation with interface type normalization (`Ethernet1` → `interface:ethernet:1`, `Port-Channel10` → `interface:port-channel:10`, `Vlan100` → `interface:vlan:100`, `Loopback0` → `interface:loopback:0`, `Management1` → `interface:management:1`, `Vxlan1` → `interface:vxlan:1`), EOS-style `vrf instance` handling, and EOS-specific constructs: `mlag configuration`, `management api`, `daemon`, `event-handler`, and `peer-filter`

## [0.5.0] - 2026-05-22

- replaced `IosLikeDialect` re-export in `netform_dialect_nxos` with a dedicated `NxosDialect` struct implementing the `Dialect` trait directly — NX-OS configs now get NX-OS-specific key-hint derivation with interface type normalization (`Ethernet1/1` → `interface:ethernet:1/1`, `port-channel10` → `interface:port-channel:10`, `Vlan100` → `interface:vlan:100`, `loopback0` → `interface:loopback:0`, `mgmt0` → `interface:mgmt:0`, `nve1` → `interface:nve:1`), NX-OS-style `vrf context` handling, and `ip access-list <name>` without the IOS-style `extended`/`standard` qualifier
- **breaking:** `diff_documents` now returns `Result<Diff, DiffError>` instead of `Diff` — the `unreachable!()` in Myers SES and `.unwrap()` calls on edit-script iterators in `engine.rs` are replaced with proper `Result`-based error propagation; callers must handle the new `DiffError` type (library code should never panic on user input)
- eliminated intermediate string allocations in `normalize_for_compare` by using `Cow<str>` — normalization steps that are no-ops (e.g. trimming a line with no trailing whitespace) no longer allocate, and the initial `to_string()` clone is deferred until a step actually modifies the line
- added `--policy-override PATH:POLICY` repeatable CLI flag — applies per-context order-policy overrides (e.g. `--policy-override 0:unordered`) to specific subtrees, completing the `OrderPolicyConfig.overrides` feature that was modeled but not reachable from the CLI
- added `--dialect auto` (now the default) — `config-diff` infers the dialect from config content using score-based heuristics (FortiOS: `config`/`edit`/`next`/`end` blocks; Junos: hierarchical braces with semicolons and set-style stanza paths; NX-OS: `feature` keyword + slot/port interfaces; EOS: non-slot Ethernet + CIDR addresses + numbered ACLs; IOS XE: extended ACLs + dotted subnet/wildcard masks); falls back to `generic` when the content is ambiguous or too short to identify

## [0.4.0] - 2026-05-06

- added `--format` CLI flag with `unified` (default) and `markdown` variants — the existing `format_markdown_report` output is now reachable from the CLI via `--format markdown`
- `config-diff` now accepts `-` as a filename to read from stdin — either position (or both) can be `-`, enabling piped workflows like `fetch-config | config-diff - running.cfg`
- replaced O(n²) per-line forward scan in parser block detection with O(n) backward precomputation — parsing large configs no longer degrades quadratically
- replaced `push_str(&format!(...))` double-allocation pattern in report formatting with `std::fmt::Write` — eliminates an intermediate `String` allocation on every formatted write in both markdown and unified diff output
- eliminated redundant path cloning in `flush_segment_fallback` — the diff engine now borrows the first path once instead of cloning it twice per fallback flush
- expanded `ios_like_key_hint` with `ip route` support: `ip route <prefix> <mask> ...` → `ip-route:<prefix>` and `ip route vrf <vrf> <prefix> ...` → `ip-route:<vrf>:<prefix>` — static route diffs now correctly match by destination prefix instead of flagging as ambiguous
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
