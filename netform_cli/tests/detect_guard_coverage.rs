//! pins the IOS XR detection guard against the dialect interface-type tables
//! it shadows.
//!
//! `netform_ir::detect` scores a four-part `rack/slot/instance/port` interface
//! name as IOS XR unless the type is one another supported dialect spells, and
//! it carries its own hand-maintained copy of those spellings — the tables
//! themselves live in `netform_dialects`, which depends on `netform_ir`, so the
//! guard cannot read them.  `netform_cli` depends on both, so it is the one
//! place they can be compared.
//!
//! without this, adding an interface type to the registry silently changes what
//! `--dialect auto` reports for that vendor's four-part names.

use netform_dialects::{DialectEntry, REGISTRY};
use netform_ir::DialectHint;
use netform_ir::detect::detect_dialect;

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
                     netform_ir::detect's guard no longer covers that table",
                    entry.name,
                );
            }
        }
    }
}

/// the same entries, at the slot depths their own dialect actually uses, must
/// still reach that dialect rather than being shadowed by the guard.
#[test]
fn the_guard_does_not_cost_the_dialects_their_own_interface_names() {
    for (entry, types) in shadowed_vendors() {
        for ty in types {
            let input = format!("interface {ty}1\n");
            assert_ne!(
                detect_dialect(&input),
                DialectHint::Named("iosxr".into()),
                "{} carries `{ty}`, and a bare `interface {ty}1` reads as IOS XR",
                entry.name,
            );
        }
    }
}
