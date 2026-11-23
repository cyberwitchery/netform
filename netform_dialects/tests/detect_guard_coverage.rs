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

/// the same entries, at the slot depths their own dialect actually uses, must
/// still reach that dialect rather than being shadowed by the guard.
///
/// a single `interface` line scores below the detection threshold, and only
/// for a name a vendor's signals spell, case-sensitively, so a bare
/// `interface {ty}1` reads as generic whatever the guard does. each name is
/// added to the `interface` lines of its vendor's sample instead, which reach
/// that vendor on their own.
#[test]
fn the_guard_does_not_cost_the_dialects_their_own_interface_names() {
    for (entry, types) in shadowed_vendors() {
        let own = DialectHint::Named(entry.name.into());
        let interfaces = entry
            .sample
            .lines()
            .filter(|line| line.starts_with("interface "))
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        assert_eq!(
            detect_dialect(&interfaces),
            own,
            "{}'s sample interfaces",
            entry.name
        );

        for ty in types {
            for suffix in ["1", "1/1", "1/0/1"] {
                let input = format!("{interfaces}interface {ty}{suffix}\n");
                assert_eq!(
                    detect_dialect(&input),
                    own,
                    "{} carries `{ty}`, and `interface {ty}{suffix}` after its sample interfaces \
                     pulls detection away from it",
                    entry.name,
                );
            }
        }
    }
}
