use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextLineSelection {
    pub content: String,
    pub total_lines: usize,
    pub start_line: usize,
    pub end_line: usize,
    /// Last line the caller asked for, clamped to `total_lines` when the selection
    /// was built. A selection that stops before this line withheld content its
    /// budget could not hold.
    pub requested_end_line: usize,
    pub line_truncated: bool,
}

/// How a line selection measures its text budget. A newline separator costs one
/// unit under both, so only the per-line measurement and the clipping rule differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BudgetUnit {
    Chars,
    Bytes,
}

impl BudgetUnit {
    fn measure(self, line: &str) -> usize {
        match self {
            Self::Chars => line.chars().count(),
            Self::Bytes => line.len(),
        }
    }
}

/// Clamped last line of a requested window: an absent `line_count` means "to the end".
///
/// `line_count` is a window size, which `select`/`select_bytes` require to be at
/// least one; both operands saturate so no caller can panic here.
fn requested_end(start_line: usize, line_count: Option<usize>, total_lines: usize) -> usize {
    line_count
        .map(|count| {
            start_line
                .saturating_add(count.saturating_sub(1))
                .min(total_lines)
        })
        .unwrap_or(total_lines)
}

impl TextLineSelection {
    pub fn select(
        text: &str,
        start_line: usize,
        line_count: Option<usize>,
        max_lines: usize,
        max_chars: usize,
    ) -> Result<Self, TextLineSelectionError> {
        Self::select_within(
            text,
            start_line,
            line_count,
            Some(max_lines),
            max_chars,
            BudgetUnit::Chars,
        )
    }

    /// Byte-counted variant of [`Self::select`] without a line cap.
    ///
    /// Callers use this when the budget bounds one read in their own storage
    /// rather than paginating by line count, so only the byte budget can end a
    /// window early.
    pub fn select_bytes(
        text: &str,
        start_line: usize,
        line_count: Option<usize>,
        max_bytes: usize,
    ) -> Result<Self, TextLineSelectionError> {
        Self::select_within(
            text,
            start_line,
            line_count,
            None,
            max_bytes,
            BudgetUnit::Bytes,
        )
    }

    fn select_within(
        text: &str,
        start_line: usize,
        line_count: Option<usize>,
        max_lines: Option<usize>,
        max_units: usize,
        unit: BudgetUnit,
    ) -> Result<Self, TextLineSelectionError> {
        assert!(
            max_units > 0,
            "text line selection requires a positive budget"
        );
        if let Some(max_lines) = max_lines {
            assert!(max_lines > 0, "text line selection requires max_lines > 0");
        }

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
        // The window the caller asked for, established once here and carried on the
        // selection: withheld content is exactly what lies between `end_line` and
        // this line, so no caller has to re-derive it from its own request.
        let requested_end = requested_end(start_line, line_count, total_lines);
        if total_lines == 0 {
            // An empty source is still addressed by line 1, which simply holds no
            // rows: reporting line 0 would make a caller's `end - start + 1` span one
            // row that does not exist, and `L0-L0` reads as a real range.
            return Ok(Self {
                content: String::new(),
                total_lines: 0,
                start_line: 1,
                end_line: 0,
                requested_end_line: requested_end,
                line_truncated: false,
            });
        }

        let capped_end = max_lines
            .map(|max_lines| start_line.saturating_add(max_lines - 1))
            .unwrap_or(requested_end)
            .min(requested_end);
        let mut content = String::new();
        let mut used = 0_usize;
        let mut returned_lines = 0_usize;
        let mut line_truncated = false;

        for line in &lines[start_line - 1..capped_end] {
            let separator = usize::from(returned_lines > 0);
            let line_units = unit.measure(line);
            if used.saturating_add(separator).saturating_add(line_units) <= max_units {
                if separator == 1 {
                    content.push('\n');
                    used += 1;
                }
                content.push_str(line);
                used += line_units;
                returned_lines += 1;
                continue;
            }

            if returned_lines == 0 {
                match unit {
                    BudgetUnit::Chars => content.extend(line.chars().take(max_units)),
                    // A byte budget can land inside a character, so the clip
                    // falls back to the nearest boundary below it.
                    BudgetUnit::Bytes => {
                        let end = line.floor_char_boundary(max_units);
                        content.push_str(&line[..end]);
                    }
                }
                returned_lines = 1;
                // Reaching this branch at all means the line did not fit the budget
                // on its own, so the clip above is what the caller gets in place of
                // the whole line.
                line_truncated = true;
            }
            break;
        }

        Ok(Self {
            content,
            total_lines,
            start_line,
            end_line: start_line + returned_lines - 1,
            requested_end_line: requested_end,
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
        if self.end_line < self.start_line {
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
    // A selection that returned no rows has nothing to number, and neither has one
    // that starts before the first line.
    if start_line == 0 || end_line < start_line {
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

    #[test]
    fn an_empty_source_is_line_one_with_no_rows() {
        // The empty selection still names a line, so a caller's span arithmetic and
        // its `L{start}-L{end}` reference stay meaningful.
        let empty = TextLineSelection::select("", 1, None, 10, 100).unwrap();
        assert_eq!(empty.start_line, 1);
        assert_eq!(empty.end_line, 0);
        assert_eq!(empty.total_lines, 0);
        assert_eq!(empty.requested_end_line, 0);
        assert_eq!(empty.returned_line_count(), 0);
        assert_eq!(empty.numbered_content(), "");
        assert_eq!(empty.next_start_line(), None);
        assert!(!empty.truncated());
    }

    #[test]
    fn a_byte_budget_clips_a_line_at_a_character_boundary() {
        // Three bytes per CJK character, so a 6-byte budget cannot hold a third.
        let clipped = TextLineSelection::select_bytes("你好世界", 1, None, 6).unwrap();
        assert_eq!(clipped.content, "你好");
        assert_eq!(clipped.content.len(), 6);
        assert!(clipped.line_truncated);
    }

    #[test]
    fn a_byte_selection_has_no_line_cap() {
        let text = "one\ntwo\nthree\nfour";
        let full = TextLineSelection::select_bytes(text, 1, None, 64).unwrap();
        assert_eq!(full.content, text);
        assert_eq!(full.end_line, 4);
        assert!(!full.truncated());

        // A window wider than the text still ends at the text's last line.
        let window = TextLineSelection::select_bytes(text, 2, Some(50), 64).unwrap();
        assert_eq!(window.content, "two\nthree\nfour");
        assert_eq!(window.end_line, 4);
        assert_eq!(window.end_line, window.requested_end_line);
        assert!(!window.line_truncated);
    }

    #[test]
    fn a_window_that_the_byte_budget_cut_short_stops_before_the_requested_end() {
        let selection =
            TextLineSelection::select_bytes("one\ntwo\nthree\nfour", 1, Some(3), 7).unwrap();
        assert_eq!(selection.content, "one\ntwo");
        assert_eq!(selection.end_line, 2);
        assert_eq!(selection.requested_end_line, 3);
        assert!(selection.end_line < selection.requested_end_line);
    }
}
