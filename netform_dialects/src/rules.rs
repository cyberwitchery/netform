//! a small, total rule language for IOS-family key hints.
//!
//! every vendor-specific key hint in the IOS family has the same shape: match a
//! head keyword, optionally require some argument positions to be one of a
//! fixed set of words, then build the key from a static prefix and zero or more
//! captured arguments joined by `:`.  [`KeyRule`] is that shape as data, and
//! [`rule_key_hint`] is its interpreter.
//!
//! # Example
//!
//! ```rust
//! use netform_dialects::rules::{ArgGuard, KeyRule, KeyRuleAction, rule_key_hint};
//! use netform_ir::parse_ios_like_parts;
//!
//! const RULES: &[KeyRule] = &[KeyRule {
//!     head: "vpc",
//!     guards: &[ArgGuard::new(0, &["domain"])],
//!     action: KeyRuleAction::key("vpc-domain", &[1]),
//! }];
//!
//! let parsed = parse_ios_like_parts("vpc domain 10");
//! assert_eq!(
//!     rule_key_hint(RULES, parsed.as_ref()),
//!     Some("vpc-domain:10".to_string()),
//! );
//! ```

use netform_ir::{ParsedLineParts, common_key_hint};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRuleAction {
    /// build the key from `prefix` and the arguments at `captures`, joined by
    /// `:`.
    Key {
        /// the literal leading segment of the key.
        prefix: &'static str,
        /// argument positions to append, in order.
        captures: &'static [usize],
    },
    /// defer to [`common_key_hint`].
    ///
    /// place this last among a head's rules to make that head fall through to
    /// the shared arms instead of yielding no hint.
    Common,
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

/// one vendor-specific key-hint rule.
///
/// rules are tried in order and the first whose head and guards match wins, so
/// a longer, more specific rule must precede a shorter one over the same head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
            KeyRuleAction::Common => 0,
        };
        guarded.max(captured)
    }

    fn applies(&self, head: &str, args: &[String]) -> bool {
        self.head == head
            && args.len() >= self.required_args()
            && self.guards.iter().all(|guard| guard.holds(args))
    }

    fn render(&self, args: &[String]) -> Option<String> {
        match self.action {
            KeyRuleAction::Key { prefix, captures } => {
                let mut key = String::from(prefix);
                for index in captures {
                    key.push(':');
                    key.push_str(args.get(*index)?);
                }
                Some(key)
            }
            KeyRuleAction::Common => None,
        }
    }
}

/// derive a key hint for `parsed` from `rules`.
///
/// a head no rule mentions falls through to [`common_key_hint`].  a head some
/// rule mentions but none matches yields no hint, unless one of that head's
/// rules is [`KeyRuleAction::Common`].
pub fn rule_key_hint(rules: &[KeyRule], parsed: Option<&ParsedLineParts>) -> Option<String> {
    let parts = parsed?;
    let head = parts.head.as_str();
    let args = parts.args.as_slice();

    let mut head_is_claimed = false;
    for rule in rules {
        if rule.head != head {
            continue;
        }
        head_is_claimed = true;
        if !rule.applies(head, args) {
            continue;
        }
        return match rule.action {
            KeyRuleAction::Key { .. } => rule.render(args),
            KeyRuleAction::Common => common_key_hint(parsed),
        };
    }

    if head_is_claimed {
        None
    } else {
        common_key_hint(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::parse_ios_like_parts;

    const RULES: &[KeyRule] = &[
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
        KeyRule {
            head: "crypto",
            guards: &[],
            action: KeyRuleAction::Common,
        },
    ];

    fn hint(line: &str) -> Option<String> {
        rule_key_hint(RULES, parse_ios_like_parts(line).as_ref())
    }

    #[test]
    fn literal_action_ignores_the_guarded_argument() {
        assert_eq!(hint("mlag configuration"), Some("mlag".into()));
    }

    #[test]
    fn a_failed_guard_yields_no_hint() {
        assert_eq!(hint("mlag peer-link Port-Channel1"), None);
    }

    #[test]
    fn a_guard_accepts_any_of_its_words() {
        assert_eq!(hint("management ssh"), None);
        assert_eq!(
            hint("management telnet vrf MGMT"),
            Some("management:telnet:vrf".into())
        );
    }

    #[test]
    fn missing_captures_yield_no_hint() {
        assert_eq!(hint("crypto pki trustpoint"), None);
    }

    #[test]
    fn a_claimed_head_with_no_matching_rule_yields_no_hint() {
        assert_eq!(hint("mlag"), None);
    }

    #[test]
    fn a_common_rule_lets_its_head_fall_through() {
        assert_eq!(
            hint("crypto ikev2 proposal PROP1"),
            Some("crypto:ikev2:proposal:PROP1".into()),
        );
    }

    #[test]
    fn an_unclaimed_head_falls_through_to_the_shared_arms() {
        assert_eq!(hint("policy-map SHAPE"), Some("policy-map:SHAPE".into()));
    }

    #[test]
    fn an_unclaimed_head_with_no_shared_arm_yields_no_hint() {
        assert_eq!(hint("hostname leaf-01"), None);
    }

    #[test]
    fn a_non_content_line_yields_no_hint() {
        assert_eq!(rule_key_hint(RULES, None), None);
    }

    #[test]
    fn a_claimed_shared_head_does_not_fall_through() {
        const CLAIMS_NTP: &[KeyRule] = &[KeyRule {
            head: "ntp",
            guards: &[ArgGuard::new(0, &["authentication"])],
            action: KeyRuleAction::key("ntp-authentication", &[1]),
        }];

        let parsed = parse_ios_like_parts("ntp server 1.2.3.4");
        assert_eq!(
            common_key_hint(parsed.as_ref()),
            Some("ntp:server:1.2.3.4".into()),
        );
        assert_eq!(rule_key_hint(CLAIMS_NTP, parsed.as_ref()), None);
    }
}
