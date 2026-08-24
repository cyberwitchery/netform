//! Juniper Junos as registry data.
//!
//! its parser needs real code and stays in `netform_dialect_junos`; its
//! detection signals and sample are data and live here.

use netform_ir::Document;
use netform_ir::detect::{MODERATE_SIGNAL, STRONG_SIGNAL, Signal, Test, WEAK_SIGNAL};

/// the patterns that make configuration text read as Junos: its top-level
/// stanza names in either the hierarchical or the `set` form, and the
/// brace-and-semicolon syntax the hierarchical form is written in.
pub const SIGNALS: &[Signal] = &[
    Signal {
        weight: STRONG_SIGNAL,
        tests: &[
            Test::StartsWithAny(&["set ", "unset "]),
            Test::WordIsJunosStanza(1),
        ],
    },
    Signal {
        weight: STRONG_SIGNAL,
        tests: &[Test::WordIsJunosStanza(0)],
    },
    Signal {
        weight: MODERATE_SIGNAL,
        tests: &[Test::EndsWithAny(&["{"])],
    },
    Signal {
        weight: MODERATE_SIGNAL,
        tests: &[Test::IsAny(&["}"])],
    },
    Signal {
        weight: MODERATE_SIGNAL,
        tests: &[Test::EndsWithAny(&["};"])],
    },
    Signal {
        weight: WEAK_SIGNAL,
        tests: &[
            Test::EndsWithAny(&[";"]),
            Test::Not(&Test::EndsWithAny(&["};"])),
        ],
    },
];

/// parse text as Junos.
pub fn parse(input: &str) -> Document {
    netform_dialect_junos::parse_junos(input)
}

/// a canonical Junos excerpt.
pub const SAMPLE: &str = "\
interfaces {
    ge-0/0/0 {
        description uplink-core-a;
        unit 0 {
            family inet {
                address 192.0.2.2/30;
            }
        }
    }
}
protocols {
    bgp {
        group underlay {
            peer-as 65001;
        }
    }
}
";
