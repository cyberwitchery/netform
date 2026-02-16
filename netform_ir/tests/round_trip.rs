use netform_ir::{DialectHint, parse_generic};

#[test]
fn round_trip_ios_style_snippet() {
    let input = "! base config\ninterface Ethernet1/1\n  description Uplink\n  ip address 10.0.0.1/31\n\nrouter bgp 65000\n  neighbor 10.0.0.0 remote-as 65001\n";

    let doc = parse_generic(input);
    assert_eq!(doc.render(), input);
    assert_eq!(doc.metadata.line_count, 7);
}

#[test]
fn round_trip_without_trailing_newline() {
    let input = "set system host-name edge-01\nset system services ssh";

    let doc = parse_generic(input);
    assert_eq!(doc.render(), input);
    assert_eq!(doc.metadata.line_count, 2);
}

#[test]
fn round_trip_mixed_line_endings() {
    let input = "hostname leaf-1\r\n! keep this\ninterface Ethernet2\r\n description mixed-eol";

    let doc = parse_generic(input);
    assert_eq!(doc.render(), input);
    assert_eq!(doc.metadata.line_count, 4);
}

#[test]
fn metadata_sets_generic_dialect_hint() {
    let doc = parse_generic("hostname leaf-1\n");
    assert_eq!(doc.metadata.dialect_hint, DialectHint::Generic);
}
