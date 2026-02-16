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
                output = output.split_whitespace().collect::<Vec<_>>().join(" ");
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
