use std::borrow::Cow;
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashSet};

const MAX_QUERY_TOKENS: usize = 64;
const MAX_BIGRAM_TOKENS_PER_SEGMENT: usize = 32;
const SNIPPET_MAX_CHARS: usize = 200;
const SNIPPET_CONTEXT_BEFORE: usize = 40;

/// The ranked word search over chat messages, shared by the chat search API and the
/// Agent's `chat.search`. The query splits into words, and a long or unspaced word also
/// into character pairs; a text scores the length-weighted share of those words it
/// contains. Offered texts compete for `limit` places.
pub struct RankedTextSearch<'a, T> {
    tokens: Vec<String>,
    needs_lowercase: bool,
    limit: usize,
    best: BinaryHeap<Reverse<Candidate<'a, T>>>,
}

/// A kept text with its score and a snippet around its first match; `item` is the
/// caller's data for it.
pub struct RankedHit<'a, T> {
    pub index: usize,
    pub score: f32,
    pub snippet: String,
    pub text: Cow<'a, str>,
    pub item: T,
}

struct Candidate<'a, T> {
    index: usize,
    score: f32,
    match_byte: Option<usize>,
    text: Cow<'a, str>,
    item: T,
}

impl<T> Candidate<'_, T> {
    fn rank(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| self.index.cmp(&other.index))
    }
}

impl<T> PartialEq for Candidate<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        self.rank(other).is_eq()
    }
}

impl<T> Eq for Candidate<'_, T> {}

impl<T> PartialOrd for Candidate<'_, T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Candidate<'_, T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank(other)
    }
}

impl<'a, T> RankedTextSearch<'a, T> {
    pub fn new(query: &str, limit: usize) -> Self {
        let tokens = build_query_tokens(query);
        Self {
            needs_lowercase: needs_ascii_lowercase(&tokens),
            tokens,
            limit,
            best: BinaryHeap::new(),
        }
    }

    /// Whether the query holds no words; such a search keeps nothing.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// Scores `text` as message `index` and keeps it while it ranks among the best.
    pub fn offer(&mut self, index: usize, text: impl Into<Cow<'a, str>>, item: T) {
        if self.tokens.is_empty() || self.limit == 0 {
            return;
        }
        let text = text.into();
        let (score, match_byte) = score_text(&text, &self.tokens, self.needs_lowercase);
        if score <= 0.0 {
            return;
        }
        let candidate = Candidate {
            index,
            score,
            match_byte,
            text,
            item,
        };
        if self.best.len() < self.limit {
            self.best.push(Reverse(candidate));
        } else if self
            .best
            .peek()
            .is_some_and(|Reverse(worst)| candidate > *worst)
        {
            self.best.pop();
            self.best.push(Reverse(candidate));
        }
    }

    /// The kept texts, best first; of equal scores the later message comes first.
    pub fn finish(self) -> Vec<RankedHit<'a, T>> {
        let mut candidates = self
            .best
            .into_iter()
            .map(|Reverse(candidate)| candidate)
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.cmp(left));
        candidates
            .into_iter()
            .map(|candidate| RankedHit {
                index: candidate.index,
                score: candidate.score,
                snippet: snippet_from_text(&candidate.text, candidate.match_byte),
                text: candidate.text,
                item: candidate.item,
            })
            .collect()
    }
}

/// Normalizes a scored text-search query without discarding non-empty literal input.
fn normalize_search_query(value: &str) -> String {
    let literal = value.trim().to_lowercase();
    let normalized = literal
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
        .join(" ");

    if normalized.is_empty() {
        literal
    } else {
        normalized
    }
}

fn bigram_tokens(value: &str, limit: usize) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    if chars.len() == 1 {
        return vec![value.to_string()];
    }

    let total = chars.len() - 1;
    let step = total.div_ceil(limit);
    let mut seen = HashSet::new();
    let mut tokens = Vec::new();

    for index in (0..total).step_by(step) {
        let token = format!("{}{}", chars[index], chars[index + 1]);
        if seen.insert(token.clone()) {
            tokens.push(token);
        }
    }

    tokens
}

fn has_word_chars(value: &str) -> bool {
    value.chars().any(|ch| ch.is_alphanumeric() || ch == '_')
}

fn expand_query_tokens(tokens: Vec<String>) -> Vec<String> {
    if tokens.is_empty() {
        return Vec::new();
    }

    if tokens.len() == 1 {
        let token = &tokens[0];
        let char_count = token.chars().count();
        if char_count <= 2 || !has_word_chars(token) {
            return tokens;
        }

        let mut expanded = bigram_tokens(token, MAX_BIGRAM_TOKENS_PER_SEGMENT);
        if char_count <= 8 && expanded.len() < MAX_BIGRAM_TOKENS_PER_SEGMENT {
            expanded.insert(0, token.clone());
        }
        return expanded;
    }

    let mut expanded = Vec::new();
    for token in tokens {
        let char_count = token.chars().count();
        let is_ascii_word = token.chars().any(|ch| ch.is_ascii_alphanumeric());
        if has_word_chars(&token) && !is_ascii_word && char_count >= 8 {
            expanded.extend(bigram_tokens(&token, 8));
        } else {
            expanded.push(token);
        }
    }

    expanded
}

fn dedup_and_limit_tokens(tokens: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for token in tokens {
        if token.is_empty() {
            continue;
        }
        if seen.insert(token.clone()) {
            unique.push(token);
        }
    }

    if unique.len() <= MAX_QUERY_TOKENS {
        return unique;
    }

    let step = unique.len().div_ceil(MAX_QUERY_TOKENS);
    unique
        .into_iter()
        .step_by(step)
        .take(MAX_QUERY_TOKENS)
        .collect()
}

fn build_query_tokens(query: &str) -> Vec<String> {
    let normalized = normalize_search_query(query);
    if normalized.is_empty() {
        return Vec::new();
    }

    let base = normalized
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let expanded = expand_query_tokens(base);
    dedup_and_limit_tokens(expanded)
}

fn token_weight(token: &str) -> usize {
    token.chars().count().clamp(1, 8)
}

fn needs_ascii_lowercase(tokens: &[String]) -> bool {
    tokens
        .iter()
        .any(|token| token.chars().any(|ch| ch.is_ascii_alphabetic()))
}

fn score_text(text: &str, tokens: &[String], needs_lowercase: bool) -> (f32, Option<usize>) {
    if text.trim().is_empty() || tokens.is_empty() {
        return (0.0, None);
    }

    let search_text;
    let haystack: &str = if needs_lowercase {
        search_text = text.to_lowercase();
        &search_text
    } else {
        text
    };

    let mut total_weight: usize = 0;
    let mut matched_weight: usize = 0;
    let mut first_match: Option<usize> = None;

    for token in tokens {
        let weight = token_weight(token);
        total_weight += weight;

        if let Some(pos) = haystack.find(token) {
            matched_weight += weight;
            first_match = match first_match {
                Some(existing) => Some(existing.min(pos)),
                None => Some(pos),
            };
        }
    }

    if total_weight == 0 || matched_weight == 0 {
        return (0.0, first_match);
    }

    let score = (matched_weight as f32) / (total_weight as f32);
    (score, first_match)
}

fn snippet_from_text(text: &str, match_byte: Option<usize>) -> String {
    let total_chars = text.chars().count();
    if total_chars <= SNIPPET_MAX_CHARS {
        return text.to_string();
    }

    if let Some(byte_index) = match_byte {
        let prefix_chars = text.get(..byte_index).unwrap_or_default().chars().count();
        let start = prefix_chars.saturating_sub(SNIPPET_CONTEXT_BEFORE);
        let end = (start + SNIPPET_MAX_CHARS).min(total_chars);
        let snippet: String = text
            .chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect();

        let mut output = String::new();
        if start > 0 {
            output.push_str("...");
        }
        output.push_str(&snippet);
        if end < total_chars {
            output.push_str("...");
        }
        return output;
    }

    let tail: String = text
        .chars()
        .rev()
        .take(SNIPPET_MAX_CHARS)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("...{}", tail)
}

#[cfg(test)]
mod tests {
    use super::{build_query_tokens, score_text};

    #[test]
    fn punctuation_and_symbol_only_queries_match_literally() {
        for (query, text) in [
            ("——", "pause——continue"),
            ("❤️", "status: ❤️"),
            ("👨‍👩‍👧", "family: 👨‍👩‍👧"),
        ] {
            let tokens = build_query_tokens(query);

            assert_eq!(tokens, vec![query]);
            assert_eq!(score_text(text, &tokens, false), (1.0, text.find(query)));
        }
    }
}
