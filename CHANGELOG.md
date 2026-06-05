# changelog

## [unreleased]

- the EOS dialect (`--dialect eos`) now derives EOS-specific key hints instead of reusing the IOS XE logic: interface names are normalized (e.g. `Ethernet1` → `interface:ethernet:1`, `Port-Channel10` → `interface:port-channel:10`, plus `Vlan100`, `Loopback0`, `Management1`, `Vxlan1`), `vrf instance` is handled EOS-style, and EOS-only constructs (`mlag configuration`, `management api`, `daemon`, `event-handler`, `peer-filter`) are recognized, reducing spurious ambiguous-key findings on EOS configs.
- **breaking:** NX-OS-only constructs (`feature`, `vpc`, `role`, `system`) are no longer recognized by the IOS XE dialect (`--dialect iosxe`); they live only in `--dialect nxos`. IOS XE configs that happened to contain these stanzas will no longer get key hints for them.

## [0.5.0] - 2026-05-22

- `--dialect auto` (now the default): `config-diff` infers the dialect from config content (FortiOS, Junos, NX-OS, EOS, IOS XE) using score-based heuristics, and falls back to `generic` when the content is ambiguous or too short to identify.
- the NX-OS dialect (`--dialect nxos`) now derives NX-OS-specific key hints instead of reusing the IOS XE logic: interface names are normalized (e.g. `Ethernet1/1` → `interface:ethernet:1/1`, `port-channel10`, `Vlan100`, `loopback0`, `mgmt0`, `nve1`), `vrf context` is handled NX-OS-style, and `ip access-list <name>` is matched without the IOS-style `extended`/`standard` qualifier.
- `--policy-override PATH:POLICY` (repeatable): apply per-context order-policy overrides to specific subtrees, e.g. `--policy-override 0:unordered`.
- **breaking:** `diff_documents` now returns `Result<Diff, DiffError>` instead of `Diff`; it no longer panics on malformed input, so callers must handle the new `DiffError`.

## [0.4.0] - 2026-05-06

- new `--dialect fortios` for Fortinet FortiOS (`config`/`edit` key hints, `#` comment classification, quoted-string tokenization).
- new `--dialect nxos` for Cisco NX-OS, recognizing NX-OS stanzas (`feature`, `vpc domain`, `role name`, `monitor session`, `ntp`, `system`).
- much wider key-hint coverage, which reduces spurious "ambiguous key" findings:
  - IOS-like: `ip route` (with VRF), IPv6 (`ipv6 access-list`, `ipv6 prefix-list`, `ipv6 route`), `class-map`, `policy-map`, `ip community-list`, numbered access-lists, crypto (`isakmp`, `map`, `ikev2`, `ipsec`), and `spanning-tree vlan`.
  - Junos: `system` (with `host-name`, `services`, `login`, `ntp`, `syslog` sub-stanzas), `routing-options`, `firewall`, `security`, `snmp`, `vlans`, `chassis`, `class-of-service`, `forwarding-options`, `applications`, and `groups`, in both hierarchical and set-style form.
  - FortiOS: `set:`/`unset:` subkey hints, so a changed field value is reported as a modification instead of a delete + add pair.
- the default output is now a colored unified diff (`---`/`+++` headers, red deletions, green insertions, cyan `@@` hunk headers) instead of markdown; colors auto-detect from the TTY and can be forced with `--color`/`--no-color`. JSON and plan-JSON output are unchanged.
- `--format unified|markdown` to choose the report format explicitly; the markdown report now shows the actual changed config lines (with `+`/`-` markers) under each edit.
- `config-diff` reads from stdin when a filename is `-` (either or both positions), enabling pipelines like `fetch-config | config-diff - running.cfg`.
- `--context-lines N` controls how many lines are shown per side of each edit in the markdown report (default 10).
- `--trim-trailing-whitespace` and `--normalize-leading-whitespace` normalization flags.
- differentiated exit codes following `diff(1)`: 0 = no differences, 1 = differences found, 2 = error; `--no-exit-code` suppresses exit 1 (but not 2).
- multiset (keyed, unordered) diffs now emit one edit per changed field instead of collapsing everything into a single block replacement, so reports pinpoint exactly which fields changed.
- plan-JSON findings now include optional `level`, `path`, and `span` fields so consumers can locate which config line triggered each diagnostic.
- text reports now print finding levels in lowercase (`warning`/`info`), matching JSON output.
- fix: a bare `*` (without a trailing space) in Junos configs is no longer misclassified as a comment, which prevented false-positive comment stripping under `IgnoreComments`.
- faster diffing of large configs: quadratic scans in the parser and the diff engine were replaced with linear and hash-based lookups.

## [0.3.0] - 2026-04-16

- `config-diff` now exits 1 when the configs differ (like `diff(1)`); use `--no-exit-code` to suppress.
- fix: `CollapseInternalWhitespace` no longer strips leading indentation, so hierarchical configs no longer produce spurious matches between lines at different nesting levels.

## [0.2.0] - 2026-02-17

- ship the `config-diff` and `netform-replay-fixtures` binaries (new `netform_cli` crate).
- diffs are now computed with a deterministic Myers edit-script algorithm, replacing the previous quadratic LCS-matrix alignment (faster, with stable output).
- documented the CLI install and run paths in the README.

## [0.1.0] - 2026-02-16

- initial version.
