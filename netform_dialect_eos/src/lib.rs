//! Arista EOS-oriented dialect profile for `netform_ir`.
//!
//! This crate provides a conservative EOS profile that customizes:
//! - comment classification (`!`, `#`)
//! - tokenization with quoted-string preservation
//!
//! # Example
//!
//! ```rust
//! use netform_dialect_eos::parse_eos;
//!
//! let cfg = "interface Ethernet1\n   description \"Uplink\"\n";
//! let doc = parse_eos(cfg);
//! assert_eq!(doc.render(), cfg);
//! ```

use netform_ir::{
    Dialect, DialectHint, Document, ParsedLineParts, TriviaKind, parse_with_dialect, tokenize,
};

/// Dialect implementation for EOS-like configuration text.
#[derive(Debug, Default, Clone, Copy)]
pub struct EosDialect;

/// Parse text using [`EosDialect`].
pub fn parse_eos(input: &str) -> Document {
    parse_with_dialect(input, &EosDialect)
}

impl Dialect for EosDialect {
    fn dialect_hint(&self) -> DialectHint {
        DialectHint::Named("eos".to_string())
    }

    fn classify_trivia(&self, raw: &str) -> TriviaKind {
        classify_eos_trivia(raw)
    }

    fn parse_parts(&self, raw: &str) -> Option<ParsedLineParts> {
        parse_eos_parts(raw)
    }

    fn key_hint(
        &self,
        _raw: &str,
        parsed: Option<&ParsedLineParts>,
        trivia: TriviaKind,
    ) -> Option<String> {
        if trivia != TriviaKind::Content {
            return None;
        }
        eos_like_key_hint(parsed)
    }
}

fn classify_eos_trivia(raw: &str) -> TriviaKind {
    if raw.trim().is_empty() {
        return TriviaKind::Blank;
    }

    let trimmed = raw.trim_start();
    if trimmed.starts_with('!') || trimmed.starts_with('#') {
        return TriviaKind::Comment;
    }

    TriviaKind::Content
}

fn parse_eos_parts(raw: &str) -> Option<ParsedLineParts> {
    let tokens = tokenize(raw, &[]);
    let head = tokens.first()?.clone();
    let args = tokens.into_iter().skip(1).collect::<Vec<_>>();
    Some(ParsedLineParts { head, args })
}

fn eos_like_key_hint(parsed: Option<&ParsedLineParts>) -> Option<String> {
    let parsed = parsed?;
    let head = parsed.head.as_str();
    let args = parsed.args.as_slice();

    match head {
        "interface" => args.first().map(|name| format!("interface:{name}")),
        "vlan" => args.first().map(|id| format!("vlan:{id}")),
        "vrf" => args.first().map(|name| format!("vrf:{name}")),
        "router" => match args {
            [proto, asn, ..] if proto == "bgp" => Some(format!("router:bgp:{asn}")),
            [proto, ..] => Some(format!("router:{proto}")),
            _ => None,
        },
        "route-map" => match args {
            [name, action, seq, ..] => Some(format!("route-map:{name}:{action}:{seq}")),
            [name, action] => Some(format!("route-map:{name}:{action}")),
            _ => None,
        },
        "ip" => match args {
            [next, kind, name, ..] if next == "access-list" => {
                Some(format!("ip-access-list:{kind}:{name}"))
            }
            [next, name, ..] if next == "prefix-list" => Some(format!("prefix-list:{name}")),
            _ => None,
        },
        "line" => match args {
            [kind, from, to, ..] => Some(format!("line:{kind}:{from}:{to}")),
            [kind, one, ..] => Some(format!("line:{kind}:{one}")),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eos_comment_classification_supports_bang_and_hash() {
        assert_eq!(classify_eos_trivia("!"), TriviaKind::Comment);
        assert_eq!(classify_eos_trivia("# generated"), TriviaKind::Comment);
        assert_eq!(classify_eos_trivia("vlan 10"), TriviaKind::Content);
    }

    #[test]
    fn eos_tokenization_keeps_quoted_values_together() {
        let parsed =
            parse_eos_parts("description \"Transit uplink\"").expect("content should parse");
        assert_eq!(parsed.head, "description");
        assert_eq!(parsed.args, vec!["\"Transit uplink\""]);
    }

    #[test]
    fn parse_eos_sets_named_dialect_hint() {
        let doc = parse_eos("hostname leaf-01\n");
        assert_eq!(doc.metadata.dialect_hint, DialectHint::Named("eos".into()));
    }
}
