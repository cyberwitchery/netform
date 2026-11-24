# netform_ir

the lossless intermediate representation (ir) under netform: a tree model of network config text,
a conservative parser, and a renderer that gives back the input byte for byte.

- structure comes from indentation, plus the terminators, keyword nesting, and literal and comment
  regions a `Dialect` declares
- lines the parser doesn't recognize stay plain lines, and no input line is dropped
- node ids are stable within one parsed document; diffs address lines by their path in it

vendor parsers live in [netform_dialects](https://crates.io/crates/netform_dialects), the diff
engine in [netform_diff](https://crates.io/crates/netform_diff).

## usage

```rust
use netform_ir::{Node, parse_generic};

let input = "interface Ethernet1\n  description uplink\n";
let doc = parse_generic(input);

// one root block: the interface line, with the description as its child
assert_eq!(doc.roots.len(), 1);
assert!(matches!(doc.node(doc.roots[0]), Some(Node::Block(_))));

assert_eq!(doc.render(), input);
```

## docs

- [ir model](https://github.com/cyberwitchery/netform/blob/main/docs/model.md)
- [project readme](https://github.com/cyberwitchery/netform#readme)
