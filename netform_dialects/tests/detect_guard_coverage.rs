//! pins the IOS XR detection guard against the dialect interface-type tables
//! it shadows.
//!
//! `netform_dialects::detect` scores a four-part `rack/slot/instance/port`
//! interface name as IOS XR unless the type is one another registered dialect
//! spells, reading those spellings from the registry's interface-type tables.
//!
//! without this, a table the guard stops reading, or a type that starts with an
//! IOS XR-only prefix, silently makes `--dialect auto` report that vendor's
//! four-part names as IOS XR.

use netform_dialects::{DialectEntry, REGISTRY, detect_dialect};
use netform_ir::DialectHint;

/// every registered vendor with an interface-type table, except IOS XR itself:
/// the guard exists to keep the *other* vendors' spellings out of IOS XR.
fn shadowed_vendors() -> impl Iterator<Item = (&'static DialectEntry, &'static [&'static str])> {
    REGISTRY
        .iter()
        .filter(|entry| entry.name != "iosxr")
        .filter_map(|entry| Some((entry, entry.rules?.interface_types)))
}

/// every interface type a non-XR dialect parses must stay out of IOS XR's
/// four-part-slot signal, whatever slot depth it is written at.
#[test]
fn no_dialect_interface_type_scores_iosxr_on_slot_shape() {
    let iosxr = DialectHint::Named("iosxr".into());

    for (entry, types) in shadowed_vendors() {
        for ty in types {
            for suffix in ["1/0/2/1", "0/0/0/0", "1/2/3/4.100"] {
                let input = format!("interface {ty}{suffix}\n");
                assert_ne!(
                    detect_dialect(&input),
                    iosxr,
                    "{} carries `{ty}`, and `interface {ty}{suffix}` reads as IOS XR — \
                     netform_dialects::detect's guard no longer covers that table",
                    entry.name,
                );
            }
        }
    }
}

/// each vendor's own `interface` lines, taken from its sample, must still reach
/// that vendor rather than being shadowed by the guard.
#[test]
fn the_guard_does_not_cost_the_dialects_their_own_interface_names() {
    for (entry, _) in shadowed_vendors() {
        let interfaces = entry
            .sample
            .lines()
            .filter(|line| line.starts_with("interface "))
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        assert_eq!(
            detect_dialect(&interfaces),
            DialectHint::Named(entry.name.into()),
            "{}'s sample interfaces",
            entry.name
        );
    }
}
