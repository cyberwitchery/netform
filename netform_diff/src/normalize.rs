use std::borrow::Cow;

use netform_ir::{TriviaKind, count_indent};

use crate::model::{NormalizationStep, NormalizeOptions};

pub(crate) fn normalize_for_compare(
    raw: &str,
    trivia: TriviaKind,
    options: &NormalizeOptions,
) -> Option<String> {
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
                let mut parts = body.split_whitespace();
                if let Some(first) = parts.next() {
                    let mut collapsed = String::with_capacity(body.len());
                    collapsed.push_str(first);
                    for word in parts {
                        collapsed.push(' ');
                        collapsed.push_str(word);
                    }
                    if collapsed.len() != body.len() {
                        output = Cow::Owned(format!("{}{collapsed}", &output[..leading_len]));
                    }
                }
            }
        }
    }

    Some(output.into_owned())
}

pub(crate) fn trivia_tag(kind: TriviaKind) -> &'static str {
    match kind {
        TriviaKind::Blank => "blank",
        TriviaKind::Comment => "comment",
        TriviaKind::Content => "content",
        TriviaKind::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NormalizationStep, NormalizeOptions};

    fn collapse(input: &str) -> String {
        let opts = NormalizeOptions::new(vec![NormalizationStep::CollapseInternalWhitespace]);
        normalize_for_compare(input, TriviaKind::Content, &opts).unwrap()
    }

    #[test]
    fn collapse_internal_whitespace_preserves_leading_indent() {
        assert_eq!(collapse("    hello   world"), "    hello world");
    }

    #[test]
    fn collapse_internal_whitespace_preserves_tab_indent() {
        assert_eq!(collapse("\thello   world"), "\thello world");
    }

    #[test]
    fn collapse_internal_whitespace_no_indent() {
        assert_eq!(collapse("hello   world"), "hello world");
    }

    #[test]
    fn collapse_internal_whitespace_only_indent() {
        assert_eq!(collapse("    "), "    ");
    }

    #[test]
    fn collapse_internal_whitespace_mixed_indent_and_runs() {
        assert_eq!(
            collapse("  description   uplink   port"),
            "  description uplink port"
        );
    }
}
