use crate::engine::{Op, compute_ops};

/// a span of text within a line, tagged with whether it differs from
/// the corresponding line in a replace pair.
pub(crate) struct TokenSpan<'a> {
    pub text: &'a str,
    pub changed: bool,
}

/// split a string into alternating whitespace / non-whitespace runs.
///
/// the concatenation of all returned slices equals the original string.
fn tokenize(s: &str) -> Vec<&str> {
    if s.is_empty() {
        return Vec::new();
    }
    let bytes = s.as_bytes();
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut in_ws = bytes[0].is_ascii_whitespace();

    for (i, &b) in bytes.iter().enumerate().skip(1) {
        let ws = b.is_ascii_whitespace();
        if ws != in_ws {
            tokens.push(&s[start..i]);
            start = i;
            in_ws = ws;
        }
    }
    tokens.push(&s[start..]);
    tokens
}

/// compute token-level inline diff between two lines.
///
/// returns a pair of span vectors for the old and new lines. each span
/// carries a `changed` flag indicating whether that token differs between
/// the two lines. unchanged tokens appear in both vectors at corresponding
/// positions; changed tokens appear only on their respective side.
///
/// falls back to marking all tokens as changed if the Myers algorithm
/// does not converge (should not happen for typical config lines).
pub(crate) fn inline_diff<'a>(
    old: &'a str,
    new: &'a str,
) -> (Vec<TokenSpan<'a>>, Vec<TokenSpan<'a>>) {
    let old_tokens = tokenize(old);
    let new_tokens = tokenize(new);

    let old_keys: Vec<u64> = old_tokens
        .iter()
        .map(|t| xxhash_rust::xxh3::xxh3_64(t.as_bytes()))
        .collect();
    let new_keys: Vec<u64> = new_tokens
        .iter()
        .map(|t| xxhash_rust::xxh3::xxh3_64(t.as_bytes()))
        .collect();

    let ops = match compute_ops(&old_keys, &new_keys) {
        Ok(ops) => ops,
        Err(_) => {
            return (
                old_tokens
                    .into_iter()
                    .map(|t| TokenSpan {
                        text: t,
                        changed: true,
                    })
                    .collect(),
                new_tokens
                    .into_iter()
                    .map(|t| TokenSpan {
                        text: t,
                        changed: true,
                    })
                    .collect(),
            );
        }
    };

    let mut old_spans = Vec::new();
    let mut new_spans = Vec::new();
    let mut oi = 0;
    let mut ni = 0;

    for op in ops {
        match op {
            Op::Equal => {
                old_spans.push(TokenSpan {
                    text: old_tokens[oi],
                    changed: false,
                });
                new_spans.push(TokenSpan {
                    text: new_tokens[ni],
                    changed: false,
                });
                oi += 1;
                ni += 1;
            }
            Op::Delete => {
                old_spans.push(TokenSpan {
                    text: old_tokens[oi],
                    changed: true,
                });
                oi += 1;
            }
            Op::Insert => {
                new_spans.push(TokenSpan {
                    text: new_tokens[ni],
                    changed: true,
                });
                ni += 1;
            }
        }
    }

    (old_spans, new_spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_empty() {
        assert!(tokenize("").is_empty());
    }

    #[test]
    fn tokenize_single_word() {
        assert_eq!(tokenize("hostname"), vec!["hostname"]);
    }

    #[test]
    fn tokenize_leading_whitespace() {
        assert_eq!(
            tokenize("  set hostname"),
            vec!["  ", "set", " ", "hostname"]
        );
    }

    #[test]
    fn tokenize_roundtrip() {
        let input = "  set hostname router1  ";
        let tokens = tokenize(input);
        let reconstructed: String = tokens.into_iter().collect();
        assert_eq!(reconstructed, input);
    }

    #[test]
    fn inline_diff_identical() {
        let (old, new) = inline_diff("set hostname router1", "set hostname router1");
        assert!(old.iter().all(|s| !s.changed));
        assert!(new.iter().all(|s| !s.changed));
    }

    #[test]
    fn inline_diff_single_token_change() {
        let (old, new) = inline_diff("set hostname old", "set hostname new");
        // "set", " ", "hostname", " " should be unchanged
        let old_changed: Vec<&str> = old.iter().filter(|s| s.changed).map(|s| s.text).collect();
        let new_changed: Vec<&str> = new.iter().filter(|s| s.changed).map(|s| s.text).collect();
        assert_eq!(old_changed, vec!["old"]);
        assert_eq!(new_changed, vec!["new"]);
    }

    #[test]
    fn inline_diff_no_common_words() {
        let (old, new) = inline_diff("permit any", "deny all");
        // word tokens differ but the whitespace separator matches
        let old_changed: Vec<&str> = old.iter().filter(|s| s.changed).map(|s| s.text).collect();
        let new_changed: Vec<&str> = new.iter().filter(|s| s.changed).map(|s| s.text).collect();
        assert_eq!(old_changed, vec!["permit", "any"]);
        assert_eq!(new_changed, vec!["deny", "all"]);
        assert!(old.iter().any(|s| !s.changed), "whitespace should match");
    }

    #[test]
    fn inline_diff_empty_old() {
        let (old, new) = inline_diff("", "set hostname");
        assert!(old.is_empty());
        assert!(new.iter().all(|s| s.changed));
    }

    #[test]
    fn inline_diff_empty_new() {
        let (old, new) = inline_diff("set hostname", "");
        assert!(old.iter().all(|s| s.changed));
        assert!(new.is_empty());
    }

    #[test]
    fn inline_diff_preserves_text() {
        let (old, new) = inline_diff("  set mtu 1500", "  set mtu 9000");
        let old_text: String = old.iter().map(|s| s.text).collect();
        let new_text: String = new.iter().map(|s| s.text).collect();
        assert_eq!(old_text, "  set mtu 1500");
        assert_eq!(new_text, "  set mtu 9000");
    }

    #[test]
    fn inline_diff_leading_whitespace_unchanged() {
        let (old, _new) = inline_diff("  set hostname old", "  set hostname new");
        // leading whitespace token should be unchanged
        assert!(!old[0].changed);
        assert_eq!(old[0].text, "  ");
    }
}
