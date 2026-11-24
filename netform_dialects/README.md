# netform_dialects

the registry of vendors netform parses: `eos`, `fortios`, `iosxe`, `iosxr`, `junos` and `nxos`.
each is one entry in `REGISTRY` with its parser and the signals that detect it from config text.
`find("generic")` returns netform_ir's vendor-neutral parser.

## usage

```rust
use netform_ir::DialectHint;

let input = "interfaces {\n    ge-0/0/0 {\n        disable;\n    }\n}\n";

// parse as a vendor by the name `config-diff --dialect` takes
let junos = netform_dialects::find("junos").expect("junos is registered");
let doc = (junos.parse)(input);
assert_eq!(doc.render(), input);

// or detect the vendor from the text
assert_eq!(
    netform_dialects::detect_dialect(input),
    DialectHint::Named("junos".into()),
);
```

`auto_parse` does both in one call, and falls back to the generic parser when no vendor is a
confident match.

## docs

- [ir model and parser behavior](https://github.com/cyberwitchery/netform/blob/main/docs/model.md)
- [project readme](https://github.com/cyberwitchery/netform#readme)
