use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextLineSelection {
    pub content: String,
    pub total_lines: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub line_truncated: bool,
}

impl TextLineSelection {
    pub fn select(
        text: &str,
        start_line: usize,
        line_count: Option<usize>,
        max_lines: usize,
        max_chars: usize,
    ) -> Result<Self, TextLineSelectionError> {
        assert!(max_lines > 0, "text line selection requires max_lines > 0");
        assert!(max_chars > 0, "text line selection requires max_chars > 0");

        if start_line == 0 {
            return Err(TextLineSelectionError::InvalidStartLine);
        }
        if line_count == Some(0) {
            return Err(TextLineSelectionError::InvalidLineCount);
        }

        let lines = if text.is_empty() {
            Vec::new()
        } else {
            text.split('\n').collect::<Vec<_>>()
        };
        let total_lines = lines.len();
        if start_line > total_lines.max(1) {
            return Err(TextLineSelectionError::StartLineOutOfRange {
                start_line,
                total_lines,
            });
        }
        if total_lines == 0 {
            return Ok(Self {
                content: String::new(),
                total_lines: 0,
                start_line: 0,
                end_line: 0,
                line_truncated: false,
            });
        }

        let requested_end = line_count
            .map(|count| start_line.saturating_add(count - 1).min(total_lines))
            .unwrap_or(total_lines);
        let capped_end = start_line.saturating_add(max_lines - 1).min(requested_end);
        let mut content = String::new();
        let mut chars = 0_usize;
        let mut returned_lines = 0_usize;
        let mut line_truncated = false;

        for line in &lines[start_line - 1..capped_end] {
            let separator_chars = usize::from(returned_lines > 0);
            let line_chars = line.chars().count();
            if chars
                .saturating_add(separator_chars)
                .saturating_add(line_chars)
                <= max_chars
            {
                if separator_chars == 1 {
                    content.push('\n');
                    chars += 1;
                }
                content.push_str(line);
                chars += line_chars;
                returned_lines += 1;
                continue;
            }

            if returned_lines == 0 {
                content.extend(line.chars().take(max_chars));
                returned_lines = 1;
                line_truncated = line_chars > max_chars;
            }
            break;
        }

        Ok(Self {
            content,
            total_lines,
            start_line,
            end_line: start_line + returned_lines - 1,
            line_truncated,
        })
    }

    pub fn truncated(&self) -> bool {
        self.line_truncated || self.start_line > 1 || self.end_line < self.total_lines
    }

    pub fn next_start_line(&self) -> Option<usize> {
        (self.end_line < self.total_lines).then_some(self.end_line + 1)
    }

    pub fn returned_line_count(&self) -> usize {
        if self.start_line == 0 {
            0
        } else {
            self.end_line - self.start_line + 1
        }
    }

    pub fn numbered_content(&self) -> String {
        format_lines_with_numbers(&self.content, self.start_line, self.end_line)
    }
}

pub fn format_lines_with_numbers(text: &str, start_line: usize, end_line: usize) -> String {
    if start_line == 0 {
        return String::new();
    }

    let lines = text.split('\n').collect::<Vec<_>>();
    format_line_slice_with_numbers(&lines, start_line, end_line)
}

pub(crate) fn format_line_slice_with_numbers(
    lines: &[&str],
    start_line: usize,
    end_line: usize,
) -> String {
    let width = end_line.to_string().len();
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| format!("{:>width$} | {}", start_line + index, line, width = width))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TextLineSelectionError {
    #[error("start_line must be >= 1")]
    InvalidStartLine,
    #[error("line_count must be >= 1")]
    InvalidLineCount,
    #[error("start_line {start_line} is beyond total lines {total_lines}")]
    StartLineOutOfRange {
        start_line: usize,
        total_lines: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::TextLineSelection;

    #[test]
    fn defaults_to_full_text_and_previews_only_when_bounded() {
        let full = TextLineSelection::select("one\ntwo", 1, None, 10, 100).unwrap();
        assert_eq!(full.content, "one\ntwo");
        assert!(!full.truncated());

        let preview = TextLineSelection::select("one\ntwo\nthree", 1, None, 10, 7).unwrap();
        assert_eq!(preview.content, "one\ntwo");
        assert_eq!(preview.next_start_line(), Some(3));
        assert!(preview.truncated());
    }

    #[test]
    fn marks_an_oversized_single_line_without_character_pagination() {
        let preview = TextLineSelection::select("abcdefgh", 1, None, 10, 4).unwrap();
        assert_eq!(preview.content, "abcd");
        assert!(preview.line_truncated);
        assert!(preview.truncated());
        assert_eq!(preview.next_start_line(), None);
    }
}
