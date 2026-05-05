use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::sync::{Arc, Mutex};

use regress::Regex;
use tokio::sync::Semaphore;

use crate::application::dto::native_regex_dto::{
    NativeRegexBatchRequestDto, NativeRegexBatchResponseDto, NativeRegexScriptDto,
    NativeRegexTaskResultDto,
};
use crate::application::errors::ApplicationError;

const CACHE_LIMIT: usize = 512;
const MAX_CONCURRENT_JOBS: usize = 2;

type RegexCacheHandle = Arc<Mutex<RegexCache>>;

pub struct NativeRegexService {
    cache: RegexCacheHandle,
    jobs: Arc<Semaphore>,
}

impl NativeRegexService {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(RegexCache::new(CACHE_LIMIT))),
            jobs: Arc::new(Semaphore::new(MAX_CONCURRENT_JOBS)),
        }
    }

    pub async fn apply_batch(
        &self,
        dto: NativeRegexBatchRequestDto,
    ) -> Result<NativeRegexBatchResponseDto, ApplicationError> {
        let permit = self.jobs.clone().acquire_owned().await.map_err(|error| {
            ApplicationError::InternalError(format!("Native regex queue closed: {error}"))
        })?;
        let cache = Arc::clone(&self.cache);

        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            apply_batch_blocking(cache, dto)
        })
        .await
        .map_err(|error| {
            ApplicationError::InternalError(format!("Native regex task failed: {error}"))
        })?
    }
}

impl Default for NativeRegexService {
    fn default() -> Self {
        Self::new()
    }
}

fn apply_batch_blocking(
    cache: RegexCacheHandle,
    dto: NativeRegexBatchRequestDto,
) -> Result<NativeRegexBatchResponseDto, ApplicationError> {
    let mut tasks = Vec::with_capacity(dto.tasks.len());

    for task in dto.tasks {
        let mut text = task.text;
        for script in task.scripts {
            text = apply_script(&cache, text, &script)?;
        }
        tasks.push(NativeRegexTaskResultDto { text });
    }

    Ok(NativeRegexBatchResponseDto { tasks })
}

fn apply_script(
    cache: &RegexCacheHandle,
    text: String,
    script: &NativeRegexScriptDto,
) -> Result<String, ApplicationError> {
    if script.pattern.is_empty() {
        return Err(script_error(script, "pattern is empty"));
    }

    let compile_flags = compile_flags(script)?;
    let regex = {
        let mut cache = cache.lock().map_err(|error| {
            ApplicationError::InternalError(format!("Native regex cache poisoned: {error}"))
        })?;
        cache
            .get_or_compile(&script.pattern, &compile_flags)
            .map_err(|error| script_error(script, format!("compile failed: {error}")))?
    };

    let global = script.global || script.flags.contains('g');
    replace_matches(&regex, &text, script, global)
}

fn compile_flags(script: &NativeRegexScriptDto) -> Result<String, ApplicationError> {
    let mut seen = Vec::new();

    for flag in script.flags.chars() {
        if seen.contains(&flag) {
            return Err(script_error(script, format!("duplicate flag '{flag}'")));
        }

        match flag {
            'g' => {}
            'i' | 'm' | 's' | 'u' | 'v' => seen.push(flag),
            _ => return Err(script_error(script, format!("unsupported flag '{flag}'"))),
        }
    }

    Ok(seen.into_iter().collect())
}

fn replace_matches(
    regex: &Regex,
    text: &str,
    script: &NativeRegexScriptDto,
    global: bool,
) -> Result<String, ApplicationError> {
    let mut output = String::with_capacity(text.len());
    let mut last_end = 0;
    let mut matched = false;

    for mat in regex.find_iter(text) {
        matched = true;
        output.push_str(&text[last_end..mat.start()]);
        append_replacement(&mut output, text, &mat, script);
        last_end = mat.end();

        if !global {
            break;
        }
    }

    if !matched {
        return Ok(text.to_string());
    }

    output.push_str(&text[last_end..]);
    Ok(output)
}

fn append_replacement(
    output: &mut String,
    text: &str,
    mat: &regress::Match,
    script: &NativeRegexScriptDto,
) {
    let mut chars = script.replacement.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' {
            output.push(ch);
            continue;
        }

        match chars.peek().copied() {
            Some(digit) if digit.is_ascii_digit() => {
                let mut index = 0usize;
                while let Some(next) = chars.peek().copied() {
                    if !next.is_ascii_digit() {
                        break;
                    }
                    chars.next();
                    index = index
                        .saturating_mul(10)
                        .saturating_add(next as usize - '0' as usize);
                }

                if let Some(range) = mat.group(index) {
                    output.push_str(&trim_capture(&text[range], &script.trim_strings));
                }
            }
            Some('<') => {
                chars.next();
                let mut name = String::new();
                let mut closed = false;

                for next in chars.by_ref() {
                    if next == '>' {
                        closed = true;
                        break;
                    }
                    name.push(next);
                }

                if closed && !name.is_empty() {
                    if let Some(range) = named_group_range(mat, &name) {
                        output.push_str(&trim_capture(&text[range], &script.trim_strings));
                    }
                } else {
                    output.push_str("$<");
                    output.push_str(&name);
                    if closed {
                        output.push('>');
                    }
                }
            }
            _ => output.push('$'),
        }
    }
}

fn named_group_range(mat: &regress::Match, name: &str) -> Option<Range<usize>> {
    mat.named_groups()
        .find_map(|(group_name, range)| (group_name == name).then_some(range).flatten())
}

fn trim_capture(value: &str, trim_strings: &[String]) -> String {
    let mut trimmed = value.to_string();

    for trim_string in trim_strings {
        if trim_string.is_empty() {
            continue;
        }
        trimmed = trimmed.replace(trim_string, "");
    }

    trimmed
}

fn script_error(script: &NativeRegexScriptDto, message: impl Into<String>) -> ApplicationError {
    let message = message.into();
    if script.script_name.trim().is_empty() {
        return ApplicationError::ValidationError(format!("Native regex script failed: {message}"));
    }

    ApplicationError::ValidationError(format!(
        "Native regex script '{}' failed: {message}",
        script.script_name
    ))
}

struct RegexCache {
    entries: HashMap<String, Regex>,
    order: VecDeque<String>,
    limit: usize,
}

impl RegexCache {
    fn new(limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            limit,
        }
    }

    fn get_or_compile(&mut self, pattern: &str, flags: &str) -> Result<Regex, regress::Error> {
        let key = cache_key(pattern, flags);
        if let Some(regex) = self.entries.get(&key) {
            return Ok(regex.clone());
        }

        let regex = Regex::with_flags(pattern, flags)?;
        self.insert(key, regex.clone());
        Ok(regex)
    }

    fn insert(&mut self, key: String, regex: Regex) {
        if self.limit == 0 {
            return;
        }

        if self.entries.len() >= self.limit {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }

        self.order.push_back(key.clone());
        self.entries.insert(key, regex);
    }
}

fn cache_key(pattern: &str, flags: &str) -> String {
    let mut key = String::with_capacity(pattern.len() + flags.len() + 1);
    key.push_str(pattern);
    key.push('\0');
    key.push_str(flags);
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(pattern: &str, flags: &str, replacement: &str) -> NativeRegexScriptDto {
        NativeRegexScriptDto {
            script_name: "test".to_string(),
            pattern: pattern.to_string(),
            flags: flags.to_string(),
            global: flags.contains('g'),
            replacement: replacement.to_string(),
            trim_strings: Vec::new(),
        }
    }

    fn apply(text: &str, script: NativeRegexScriptDto) -> String {
        let cache = Arc::new(Mutex::new(RegexCache::new(8)));
        apply_script(&cache, text.to_string(), &script).expect("regex apply")
    }

    #[test]
    fn replaces_first_match_without_global_flag() {
        let result = apply("a1 b2", script(r"\d", "", "X"));

        assert_eq!(result, "aX b2");
    }

    #[test]
    fn replaces_all_matches_with_global_flag() {
        let result = apply("a1 b2", script(r"\d", "g", "X"));

        assert_eq!(result, "aX bX");
    }

    #[test]
    fn supports_numbered_and_named_groups() {
        let result = apply(
            "hello world",
            script(r"(?<first>\w+)\s+(\w+)", "", "$2 $<first>"),
        );

        assert_eq!(result, "world hello");
    }

    #[test]
    fn supports_match_alias_replacement() {
        let result = apply("abc", script(r"b", "", "[$0]"));

        assert_eq!(result, "a[b]c");
    }

    #[test]
    fn preserves_literal_dollar_replacements() {
        let result = apply("abc", script(r"b", "", "$$"));

        assert_eq!(result, "a$$c");
    }

    #[test]
    fn trims_capture_replacements() {
        let mut regex = script(r"<x>([\s\S]*?)</x>", "", "$1");
        regex.trim_strings = vec!["remove".to_string()];

        let result = apply("a <x>keep remove</x> z", regex);

        assert_eq!(result, "a keep  z");
    }
}
