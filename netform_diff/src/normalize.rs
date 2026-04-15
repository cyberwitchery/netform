use netform_ir::TriviaKind;

use crate::model::{NormalizationStep, NormalizeOptions};

pub(crate) fn normalize_for_compare(
    raw: &str,
    trivia: TriviaKind,
    options: &NormalizeOptions,
) -> Option<String> {
    let mut output = raw.to_string();

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
                output = output.trim_end().to_string();
            }
            NormalizationStep::NormalizeLeadingWhitespace => {
                let indent = count_indent_columns(&output);
                let body = output.trim_start_matches([' ', '\t']).to_string();
                output = format!("{}{}", " ".repeat(indent), body);
            }
            NormalizationStep::CollapseInternalWhitespace => {
                let leading_len = output.len() - output.trim_start().len();
                let prefix = output[..leading_len].to_string();
                let collapsed = output[leading_len..]
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                output = format!("{prefix}{collapsed}");
            }
        }
    }

    Some(output)
}

fn count_indent_columns(raw: &str) -> usize {
    let mut width = 0usize;
    for ch in raw.chars() {
        match ch {
            ' ' => width += 1,
            '\t' => width += 4,
            _ => break,
        }
    }
    width
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
