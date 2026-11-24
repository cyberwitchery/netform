# netform_cli

`config-diff`, a drift report for network configs. it parses two config files with the dialect it
detects from their content, or the one `--dialect` names, diffs them structurally, and prints a
unified or markdown report, or `diff.json` / `plan.json` for tooling.

## install

```bash
cargo install netform_cli
```

## usage

```text
$ cat before.cfg
hostname leaf-01
interface Ethernet1
   description uplink
   mtu 9214
$ cat after.cfg
hostname leaf-01
interface Ethernet1
   description uplink-spine-a
   mtu 9214
$ config-diff before.cfg after.cfg
--- before.cfg
+++ after.cfg
@@ replace 1 line(s) at key 0x7c36f9c5e18e3126 -> 1 line(s) at key 0x20f7c5de56d96eaf @@
-    description uplink
+    description uplink-spine-a
$ echo $?
1
```

config-diff exits 0 when the configs match, 1 when they differ (like `diff(1)`) and 2 on an error.
`--no-exit-code` turns the 1 into a 0. `--format markdown` prints a markdown report, `--json` and
`--plan-json` print `diff.json` and `plan.json`, and `config-diff --help` lists every option.

## docs

- [cli options](https://github.com/cyberwitchery/netform#cli)
- [diff model](https://github.com/cyberwitchery/netform/blob/main/docs/diff.md)
- [heavy config example](https://github.com/cyberwitchery/netform/blob/main/docs/heavy-example.md)
