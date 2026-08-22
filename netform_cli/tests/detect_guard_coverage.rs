//! pins the IOS XR detection guard against the dialect interface-type tables
//! it shadows.
//!
//! `netform_ir::detect` scores a four-part `rack/slot/instance/port` interface
//! name as IOS XR unless the type is one another supported dialect spells, and
//! it carries its own hand-maintained copy of those spellings — the tables
//! themselves live in the dialect crates, which depend on `netform_ir`, so the
//! guard cannot read them.  `netform_cli` depends on every dialect crate and on
//! `netform_ir`, so it is the one place the two can be compared.
//!
//! without this, adding an interface type to a dialect crate silently changes
//! what `--dialect auto` reports for that vendor's four-part names.

use netform_dialect_eos::EOS_INTERFACE_TYPES;
use netform_dialect_iosxe::IOSXE_INTERFACE_TYPES;
use netform_dialect_nxos::NXOS_INTERFACE_TYPES;
use netform_dialect_vrp::VRP_INTERFACE_TYPES;
use netform_ir::DialectHint;
use netform_ir::detect::detect_dialect;

const TABLES: [(&str, &[&str]); 3] = [
    ("IOSXE_INTERFACE_TYPES", IOSXE_INTERFACE_TYPES),
    ("NXOS_INTERFACE_TYPES", NXOS_INTERFACE_TYPES),
    ("EOS_INTERFACE_TYPES", EOS_INTERFACE_TYPES),
];

/// every interface type a non-XR dialect parses must stay out of IOS XR's
/// four-part-slot signal, whatever slot depth it is written at.
#[test]
fn no_dialect_interface_type_scores_iosxr_on_slot_shape() {
    let iosxr = DialectHint::Named("iosxr".into());

    for (table, types) in TABLES {
        for ty in types {
            for suffix in ["1/0/2/1", "0/0/0/0", "1/2/3/4.100"] {
                let input = format!("interface {ty}{suffix}\n");
                assert_ne!(
                    detect_dialect(&input),
                    iosxr,
                    "{table} carries `{ty}`, and `interface {ty}{suffix}` reads as IOS XR — \
                     netform_ir::detect's guard no longer covers that table",
                );
            }
        }
    }
}

/// the same entries, at the slot depths their own dialect actually uses, must
/// still reach that dialect rather than being shadowed by the guard.
#[test]
fn the_guard_does_not_cost_the_dialects_their_own_interface_names() {
    for (table, types) in TABLES {
        for ty in types {
            let input = format!("interface {ty}1\n");
            assert_ne!(
                detect_dialect(&input),
                DialectHint::Named("iosxr".into()),
                "{table} carries `{ty}`, and a bare `interface {ty}1` reads as IOS XR",
            );
        }
    }
}

/// VRP is absent from `TABLES`: covering it at four-part depth would widen the
/// guard past `Pos0/1/0/0`, which IOS XR spells too, so its own bare and
/// three-part depths are pinned here instead.
#[test]
fn no_vrp_interface_type_scores_iosxr_at_the_depths_vrp_writes() {
    let iosxr = DialectHint::Named("iosxr".into());

    for ty in VRP_INTERFACE_TYPES {
        for suffix in ["1", "0/0/1", "0/0/1.100"] {
            let input = format!("interface {ty}{suffix}\n");
            assert_ne!(
                detect_dialect(&input),
                iosxr,
                "VRP_INTERFACE_TYPES carries `{ty}`, and `interface {ty}{suffix}` reads as IOS XR",
            );
        }
    }
}
