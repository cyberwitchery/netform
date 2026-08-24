//! Fortinet FortiOS as registry data.
//!
//! its parser needs real code and stays in `netform_dialect_fortios`; its
//! detection signals and sample are data and live here.

use netform_ir::Document;
use netform_ir::detect::{MODERATE_SIGNAL, STRONG_SIGNAL, Signal, Test, WEAK_SIGNAL};

/// the patterns that make configuration text read as FortiOS: its
/// `config`/`edit` block structure, the bare `end`/`next` terminators that
/// close them, and plain `set <field> <value>` assignments.
pub const SIGNALS: &[Signal] = &[
    Signal {
        weight: STRONG_SIGNAL,
        tests: &[
            Test::StartsWithAny(&["config "]),
            Test::Not(&Test::ContainsAny(&["{"])),
            Test::MinWords(2),
        ],
    },
    Signal {
        weight: STRONG_SIGNAL,
        tests: &[Test::StartsWithAny(&["edit "])],
    },
    Signal {
        weight: MODERATE_SIGNAL,
        tests: &[Test::IsAny(&["end", "next"])],
    },
    Signal {
        weight: WEAK_SIGNAL,
        tests: &[
            Test::StartsWithAny(&["set ", "unset "]),
            Test::Not(&Test::WordIsJunosStanza(1)),
        ],
    },
];

/// parse text as FortiOS.
pub fn parse(input: &str) -> Document {
    netform_dialect_fortios::parse_fortios(input)
}

/// a canonical FortiOS excerpt.
pub const SAMPLE: &str = "\
config system global
    set hostname \"fw-edge-01\"
    set timezone 26
end
config firewall address
    edit \"LAN\"
        set subnet 10.0.0.0 255.255.255.0
    next
end
";
