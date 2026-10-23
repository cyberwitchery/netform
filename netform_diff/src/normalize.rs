use std::borrow::Cow;
use std::ops::Range;

use netform_ir::{TriviaKind, count_indent, ends_inside_quoted_value, quoted_spans};

use crate::model::{NormalizationStep, NormalizeOptions};

pub(crate) fn normalize_for_compare(
    raw: &str,
    trivia: TriviaKind,
    quoted_values: bool,
    options: &NormalizeOptions,
) -> Option<String> {
    // no step may drop or rewrite opaque literal text.
    if trivia == TriviaKind::Literal {
        return Some(raw.to_string());
    }

    let mut output: Cow<'_, str> = Cow::Borrowed(raw);

    for step in &options.steps {
        match step {
            NormalizationStep::IgnoreComments => {
                if trivia == TriviaKind::Comment {
                    return None;
                }
            }
            NormalizationStep::IgnoreBlankLines => {
                if output.trim().is_empty() {
                    return None;
                }
            }
            NormalizationStep::TrimTrailingWhitespace => {
                if quoted_values && ends_inside_quoted_value(&output) {
                    continue;
                }
                let trimmed_len = output.trim_end().len();
                if trimmed_len < output.len() {
                    match &mut output {
                        Cow::Borrowed(s) => *s = &s[..trimmed_len],
                        Cow::Owned(s) => s.truncate(trimmed_len),
                    }
                }
            }
            NormalizationStep::NormalizeLeadingWhitespace => {
                let indent = count_indent(&output);
                let body = output.trim_start_matches([' ', '\t']);
                let leading_byte_len = output.len() - body.len();
                // no-op when leading whitespace is already canonical spaces.
                // count_indent returns 1 per space, 4 per tab, so byte_len == indent
                // implies every leading byte is a space.
                if leading_byte_len != indent {
                    output = Cow::Owned(format!(
                        "{}{}",
                        " ".repeat(indent),
                        &output[leading_byte_len..]
                    ));
                }
            }
            NormalizationStep::CollapseInternalWhitespace => {
                let leading_len = output.len() - output.trim_start().len();
                let body = &output[leading_len..];
                let values: Vec<Range<usize>> = if quoted_values {
                    quoted_spans(body).collect()
                } else {
                    Vec::new()
                };
                let collapsed = collapse_whitespace(body, &values);
                if collapsed != body {
                    output = Cow::Owned(format!("{}{collapsed}", &output[..leading_len]));
                }
            }
        }
    }

    Some(output.into_owned())
}

/// join the words of `body` with single spaces, keeping each of `values` as it is.
fn collapse_whitespace(body: &str, values: &[Range<usize>]) -> String {
    let mut collapsed = String::with_capacity(body.len());
    let mut values = values.iter().peekable();
    let mut gap = false;
    for (idx, ch) in body.char_indices() {
        // `values` are in order, so skip the ones already behind `idx`.
        while values.next_if(|value| value.end <= idx).is_some() {}
        if ch.is_whitespace() && !values.peek().is_some_and(|value| value.contains(&idx)) {
            gap = !collapsed.is_empty();
        } else {
            if gap {
                collapsed.push(' ');
                gap = false;
            }
            collapsed.push(ch);
        }
    }
    collapsed
}

pub(crate) fn trivia_tag(kind: TriviaKind) -> &'static str {
    match kind {
        TriviaKind::Blank => "blank",
        TriviaKind::Comment => "comment",
        TriviaKind::Content => "content",
        TriviaKind::Unknown => "unknown",
        TriviaKind::Literal => "literal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NormalizationStep, NormalizeOptions};

    fn collapse(input: &str) -> String {
        let opts = NormalizeOptions::new(vec![NormalizationStep::CollapseInternalWhitespace]);
        normalize_for_compare(input, TriviaKind::Content, false, &opts).unwrap()
    }

    #[test]
    fn collapse_internal_whitespace_cases() {
        for (input, expected) in [
            // leading indentation is kept (0.3.0 fix)
            ("    hello   world", "    hello world"),
            ("\thello   world", "\thello world"),
            ("hello   world", "hello world"),
            ("    ", "    "),
            ("  description   uplink   port", "  description uplink port"),
            // a single tab separator becomes a space (0.10.1)
            ("interface\tGi0/1", "interface Gi0/1"),
            ("set\tx\ty", "set x y"),
            ("  set\tx y\tz", "  set x y z"),
            ("a\x0bb\x0cc", "a b c"),
            ("  description uplink", "  description uplink"),
        ] {
            assert_eq!(collapse(input), expected, "{input:?}");
        }
    }

    #[test]
    fn collapse_internal_whitespace_keeps_every_quoted_value() {
        let opts = NormalizeOptions::new(vec![NormalizationStep::CollapseInternalWhitespace]);
        for (input, expected) in [
            (
                r#"    set member "a"  "b  c"   "d   e"  f"#,
                r#"    set member "a" "b  c" "d   e" f"#,
            ),
            (r#"set x "a  b""c  d"  y"#, r#"set x "a  b""c  d" y"#),
            (r#"set x ""  "a  b"  ""  z"#, r#"set x "" "a  b" "" z"#),
            ("set x \"a\"\t\tb", "set x \"a\" b"),
            (r#"set x "a  b" "c  d  "#, r#"set x "a  b" "c  d  "#),
        ] {
            let collapsed = normalize_for_compare(input, TriviaKind::Content, true, &opts);
            assert_eq!(collapsed.as_deref(), Some(expected), "{input:?}");
        }
    }
}
