#[derive(Debug, Clone)]
pub struct PreparedTextSearch {
    tokens: Vec<String>,
    needs_lowercase: bool,
    limit: usize,
    context_lines: usize,
}

#[derive(Debug, Clone)]
pub struct TextSearchHit {
    pub score: f32,
    pub start_line: usize,
    pub end_line: usize,
    pub matched_line: usize,
    pub snippet: String,
}

const MAX_QUERY_TOKENS: usize = 64;

impl PreparedTextSearch {
    pub fn new(query: &str, limit: usize, context_lines: usize) -> Self {
        let tokens = build_query_tokens(query);
        let needs_lowercase = tokens
            .iter()
            .any(|token| token.chars().any(|ch| ch.is_ascii_alphabetic()));
        Self {
            tokens,
            needs_lowercase,
            limit,
            context_lines,
        }
    }

    pub fn search(&self, text: &str) -> Vec<TextSearchHit> {
        if self.tokens.is_empty() || self.limit == 0 || text.trim().is_empty() {
            return Vec::new();
        }

        let lines = split_lines(text);
        let mut candidates = lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| {
                let (score, matched) = score_text(line, &self.tokens, self.needs_lowercase);
                if !matched {
                    return None;
                }

                let line_number = index + 1;
                let start_line = line_number.saturating_sub(self.context_lines).max(1);
                let end_line = (line_number + self.context_lines).min(lines.len());
                Some(TextSearchHit {
                    score,
                    start_line,
                    end_line,
                    matched_line: line_number,
                    snippet: format_lines_with_numbers(
                        &lines[start_line - 1..end_line],
                        start_line,
                    ),
                })
            })
            .collect::<Vec<_>>();

        candidates.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.matched_line.cmp(&right.matched_line))
        });

        let mut selected = Vec::new();
        for candidate in candidates {
            if selected.iter().any(|hit: &TextSearchHit| {
                ranges_overlap(
                    hit.start_line,
                    hit.end_line,
                    candidate.start_line,
                    candidate.end_line,
                )
            }) {
                continue;
            }
            selected.push(candidate);
            if selected.len() >= self.limit {
                break;
            }
        }
        selected
    }
}

fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').collect()
    }
}

fn normalize_query(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == '_' {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_query_tokens(query: &str) -> Vec<String> {
    let normalized = normalize_query(query);
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut tokens = Vec::new();
    for token in normalized.split_whitespace() {
        if token.is_empty() || tokens.iter().any(|existing| existing == token) {
            continue;
        }
        tokens.push(token.to_string());
        if tokens.len() >= MAX_QUERY_TOKENS {
            break;
        }
    }
    tokens
}

fn score_text(text: &str, tokens: &[String], needs_lowercase: bool) -> (f32, bool) {
    let search_text;
    let haystack = if needs_lowercase {
        search_text = text.to_lowercase();
        search_text.as_str()
    } else {
        text
    };

    let mut total_weight = 0_usize;
    let mut matched_weight = 0_usize;
    for token in tokens {
        let weight = token.chars().count().min(8).max(1);
        total_weight += weight;
        if haystack.contains(token) {
            matched_weight += weight;
        }
    }

    if matched_weight == 0 || total_weight == 0 {
        return (0.0, false);
    }
    (matched_weight as f32 / total_weight as f32, true)
}

fn format_lines_with_numbers(lines: &[&str], start_line: usize) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let last_line = start_line + lines.len() - 1;
    let width = last_line.to_string().len();
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| format!("{:>width$} | {}", start_line + index, line, width = width))
        .collect::<Vec<_>>()
        .join("\n")
}

fn ranges_overlap(
    left_start: usize,
    left_end: usize,
    right_start: usize,
    right_end: usize,
) -> bool {
    left_start <= right_end && right_start <= left_end
}

#[cfg(test)]
mod tests {
    use super::PreparedTextSearch;

    #[test]
    fn returns_line_numbered_snippets() {
        let search = PreparedTextSearch::new("blue lantern", 5, 1);
        let hits = search.search("alpha\nblue lantern under bridge\nomega");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].start_line, 1);
        assert_eq!(hits[0].end_line, 3);
        assert!(hits[0].snippet.contains("2 | blue lantern under bridge"));
    }

    #[test]
    fn searches_case_insensitive_ascii() {
        let search = PreparedTextSearch::new("Lantern", 5, 0);
        let hits = search.search("the blue lantern");

        assert_eq!(hits.len(), 1);
    }
}
