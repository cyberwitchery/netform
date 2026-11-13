//! a small, total rule language for IOS-family key hints.
//!
//! every vendor-specific key hint in the IOS family has the same shape: match a
//! head keyword, optionally require some argument positions to be one of a
//! fixed set of words, then build the key from a static prefix and zero or more
//! captured arguments joined by `:`.  [`KeyRule`] is that shape as data, and
//! [`IosRules::key_hint`] walks a vendor's rules, then
//! [`SHARED_RULES`](crate::ios_like::SHARED_RULES), and the first rule that
//! matches a line decides its hint.
//!
//! # Example
//!
//! ```rust
//! use netform_dialects::IosRules;
//! use netform_dialects::rules::{ArgGuard, KeyRule, KeyRuleAction};
//! use netform_ir::parse_parts_with_punctuation;
//!
//! const RULES: IosRules = IosRules {
//!     interface_types: &["ethernet"],
//!     vrf_keyword: "context",
//!     extra_router_protos: &[],
//!     route_distance_is_identity: false,
//!     key_rules: &[KeyRule {
//!         head: "vpc",
//!         guards: &[ArgGuard::new(0, &["domain"])],
//!         action: KeyRuleAction::key("vpc-domain", &[1]),
//!     }],
//! };
//!
//! let hint = |line| RULES.key_hint(parse_parts_with_punctuation(line, &[]).as_ref());
//! assert_eq!(hint("vpc domain 10"), Some("vpc-domain:10".to_string()));
//! assert_eq!(hint("interface Ethernet1"), Some("interface:ethernet:1".to_string()));
//! ```

use crate::IosRules;

/// a requirement that the argument at `index` is one of `any_of`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgGuard {
    /// position in the argument list, counting from the first word after the
    /// head keyword.
    pub index: usize,
    /// the words this position accepts.
    pub any_of: &'static [&'static str],
}

impl ArgGuard {
    /// require the argument at `index` to be one of `any_of`.
    pub const fn new(index: usize, any_of: &'static [&'static str]) -> Self {
        Self { index, any_of }
    }

    fn holds(&self, args: &[String]) -> bool {
        match args.get(self.index) {
            Some(arg) => self.any_of.contains(&arg.as_str()),
            None => false,
        }
    }
}

/// what a matching [`KeyRule`] produces.
#[derive(Debug, Clone, Copy)]
pub enum KeyRuleAction {
    /// build the key from `prefix` and the arguments at `captures`, joined by
    /// `:`.
    Key {
        /// the literal leading segment of the key.
        prefix: &'static str,
        /// argument positions to append, in order.
        captures: &'static [usize],
    },
    /// derive the key from the arguments and the vendor's [`IosRules`]; a
    /// `None` from it is the line's answer.
    Derive(fn(&[String], &IosRules) -> Option<String>),
}

impl KeyRuleAction {
    /// build the key from `prefix` and the arguments at `captures`.
    pub const fn key(prefix: &'static str, captures: &'static [usize]) -> Self {
        Self::Key { prefix, captures }
    }

    /// build the key from `prefix` alone.
    pub const fn literal(prefix: &'static str) -> Self {
        Self::Key {
            prefix,
            captures: &[],
        }
    }
}

/// one key-hint rule.
///
/// rules are tried in order and the first whose head and guards match wins, so
/// a longer, more specific rule must precede a shorter one over the same head.
#[derive(Debug, Clone, Copy)]
pub struct KeyRule {
    /// the head keyword this rule applies to.
    pub head: &'static str,
    /// requirements on argument positions.
    pub guards: &'static [ArgGuard],
    /// what to produce when the rule matches.
    pub action: KeyRuleAction,
}

impl KeyRule {
    /// the number of arguments the line must carry for this rule to apply.
    fn required_args(&self) -> usize {
        let guarded = self.guards.iter().map(|g| g.index + 1).max().unwrap_or(0);
        let captured = match self.action {
            KeyRuleAction::Key { captures, .. } => {
                captures.iter().map(|i| i + 1).max().unwrap_or(0)
            }
            KeyRuleAction::Derive(_) => 0,
        };
        guarded.max(captured)
    }

    pub(crate) fn applies(&self, head: &str, args: &[String]) -> bool {
        self.head == head
            && args.len() >= self.required_args()
            && self.guards.iter().all(|guard| guard.holds(args))
    }

    pub(crate) fn render(&self, args: &[String], vendor: &IosRules) -> Option<String> {
        match self.action {
            KeyRuleAction::Key { prefix, captures } => {
                let mut key = String::from(prefix);
                for index in captures {
                    key.push(':');
                    key.push_str(args.get(*index)?);
                }
                Some(key)
            }
            KeyRuleAction::Derive(derive) => derive(args, vendor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::parse_parts_with_punctuation;

    const fn vendor(key_rules: &'static [KeyRule]) -> IosRules {
        IosRules {
            interface_types: &[],
            vrf_keyword: "vrf",
            extra_router_protos: &[],
            route_distance_is_identity: false,
            key_rules,
        }
    }

    const RULES: IosRules = vendor(&[
        KeyRule {
            head: "mlag",
            guards: &[ArgGuard::new(0, &["configuration"])],
            action: KeyRuleAction::literal("mlag"),
        },
        KeyRule {
            head: "management",
            guards: &[ArgGuard::new(0, &["ssh", "telnet"])],
            action: KeyRuleAction::key("management", &[0, 1]),
        },
        KeyRule {
            head: "crypto",
            guards: &[ArgGuard::new(0, &["pki"])],
            action: KeyRuleAction::key("crypto:pki", &[1, 2]),
        },
    ]);

    fn hint(rules: &IosRules, line: &str) -> Option<String> {
        rules.key_hint(parse_parts_with_punctuation(line, &[]).as_ref())
    }

    #[test]
    fn rule_key_hints() {
        for (line, expected) in [
            // a literal action ignores the guarded argument.
            ("mlag configuration", Some("mlag")),
            // a failed guard on a head no shared rule covers yields no hint.
            ("mlag peer-link Port-Channel1", None),
            ("mlag", None),
            // a guard accepts any of its words; too few args yields no hint.
            ("management ssh", None),
            ("management telnet vrf MGMT", Some("management:telnet:vrf")),
            // missing captures fall through to the shared rule.
            ("crypto pki trustpoint", None),
            (
                "crypto ikev2 proposal PROP1",
                Some("crypto:ikev2:proposal:PROP1"),
            ),
            // a head no vendor rule mentions goes to the shared rules.
            ("policy-map SHAPE", Some("policy-map:SHAPE")),
            ("hostname leaf-01", None),
        ] {
            assert_eq!(hint(&RULES, line).as_deref(), expected, "{line:?}");
        }
    }

    #[test]
    fn a_non_content_line_yields_no_hint() {
        assert_eq!(RULES.key_hint(None), None);
    }

    #[test]
    fn a_vendor_rule_deriving_no_key_keeps_its_head_from_the_shared_rule() {
        fn no_key(_: &[String], _: &IosRules) -> Option<String> {
            None
        }
        const AUTHENTICATION: KeyRule = KeyRule {
            head: "ntp",
            guards: &[ArgGuard::new(0, &["authentication"])],
            action: KeyRuleAction::key("ntp-authentication", &[1]),
        };
        const FALLS_THROUGH: IosRules = vendor(&[AUTHENTICATION]);
        const CLAIMS_NTP: IosRules = vendor(&[
            AUTHENTICATION,
            KeyRule {
                head: "ntp",
                guards: &[],
                action: KeyRuleAction::Derive(no_key),
            },
        ]);

        for rules in [&FALLS_THROUGH, &CLAIMS_NTP] {
            assert_eq!(
                hint(rules, "ntp authentication 1").as_deref(),
                Some("ntp-authentication:1"),
            );
        }
        assert_eq!(
            hint(&FALLS_THROUGH, "ntp server 1.2.3.4").as_deref(),
            Some("ntp:server:1.2.3.4"),
        );
        assert_eq!(hint(&CLAIMS_NTP, "ntp server 1.2.3.4"), None);
    }
}
