//! CSS compatibility utilities for legacy WebViews.
//!
//! **Purpose**
//! - Provide a lightweight, dependency-free fallback for CSS Cascade Layers (`@layer`) on older
//!   WebViews (most commonly older Android WebView) that would otherwise ignore rules wrapped in
//!   `@layer` blocks.
//!
//! **Linkage / Data Flow**
//! - Frontend runtime (`src/scripts/extensions/runtime/third-party-runtime.js`) detects missing
//!   `@layer` support and appends `ttCompat=layer` to third-party extension stylesheet URLs.
//! - Backend web resource endpoint (`src-tauri/src/presentation/web_resources/third_party_endpoint.rs`)
//!   recognizes that query and, for `text/css` responses, uses this module to flatten `@layer`
//!   wrappers before returning bytes.
//!
//! This keeps the browser resource contract intact (`<link rel="stylesheet" href="...">` still
//! loads a real CSS response) while moving expensive preprocessing out of the WebView.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanState {
    Normal,
    Comment,
    SingleQuote,
    DoubleQuote,
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn ascii_lower(byte: u8) -> u8 {
    if byte.is_ascii_uppercase() {
        byte.to_ascii_lowercase()
    } else {
        byte
    }
}

fn matches_layer_keyword(bytes: &[u8]) -> bool {
    bytes.len() == 5
        && ascii_lower(bytes[0]) == b'l'
        && ascii_lower(bytes[1]) == b'a'
        && ascii_lower(bytes[2]) == b'y'
        && ascii_lower(bytes[3]) == b'e'
        && ascii_lower(bytes[4]) == b'r'
}

fn find_layer_at_rule_end(source: &[u8], mut index: usize) -> Option<LayerAtRuleEnd> {
    let mut state = ScanState::Normal;
    let mut escape = false;
    let mut paren_depth: u32 = 0;
    let mut bracket_depth: u32 = 0;

    while index < source.len() {
        let byte = source[index];
        match state {
            ScanState::Comment => {
                if byte == b'*' && index + 1 < source.len() && source[index + 1] == b'/' {
                    state = ScanState::Normal;
                    index += 2;
                    continue;
                }
                index += 1;
            }
            ScanState::SingleQuote => {
                if escape {
                    escape = false;
                    index += 1;
                    continue;
                }

                if byte == b'\\' {
                    escape = true;
                    index += 1;
                    continue;
                }

                if byte == b'\'' {
                    state = ScanState::Normal;
                }
                index += 1;
            }
            ScanState::DoubleQuote => {
                if escape {
                    escape = false;
                    index += 1;
                    continue;
                }

                if byte == b'\\' {
                    escape = true;
                    index += 1;
                    continue;
                }

                if byte == b'"' {
                    state = ScanState::Normal;
                }
                index += 1;
            }
            ScanState::Normal => {
                if byte == b'/' && index + 1 < source.len() && source[index + 1] == b'*' {
                    state = ScanState::Comment;
                    index += 2;
                    continue;
                }

                if byte == b'\'' {
                    state = ScanState::SingleQuote;
                    escape = false;
                    index += 1;
                    continue;
                }

                if byte == b'"' {
                    state = ScanState::DoubleQuote;
                    escape = false;
                    index += 1;
                    continue;
                }

                match byte {
                    b'(' => paren_depth = paren_depth.saturating_add(1),
                    b')' => paren_depth = paren_depth.saturating_sub(1),
                    b'[' => bracket_depth = bracket_depth.saturating_add(1),
                    b']' => bracket_depth = bracket_depth.saturating_sub(1),
                    b';' if paren_depth == 0 && bracket_depth == 0 => {
                        return Some(LayerAtRuleEnd::StatementEnd(index));
                    }
                    b'{' if paren_depth == 0 && bracket_depth == 0 => {
                        return Some(LayerAtRuleEnd::BlockStart(index));
                    }
                    _ => {}
                }

                index += 1;
            }
        }
    }

    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayerAtRuleEnd {
    StatementEnd(usize),
    BlockStart(usize),
}

fn try_parse_layer_at_rule(source: &[u8], at_index: usize) -> Option<LayerAtRuleEnd> {
    if at_index >= source.len() || source[at_index] != b'@' {
        return None;
    }

    let keyword_start = at_index + 1;
    let keyword_end = keyword_start + 5;
    if keyword_end > source.len() {
        return None;
    }

    if !matches_layer_keyword(&source[keyword_start..keyword_end]) {
        return None;
    }

    if keyword_end < source.len() && is_ident_continue(source[keyword_end]) {
        return None;
    }

    find_layer_at_rule_end(source, keyword_end)
}

/// Checks whether a stylesheet contains the `@layer` keyword in ASCII case-insensitive form.
pub fn contains_layer_keyword(source: &[u8]) -> bool {
    let mut index = 0usize;
    while index + 6 <= source.len() {
        if source[index] != b'@' {
            index += 1;
            continue;
        }

        if matches_layer_keyword(&source[index + 1..index + 6]) {
            return true;
        }

        index += 1;
    }

    false
}

/// Flattens CSS cascade layers by removing `@layer ... { ... }` wrappers.
///
/// This is intended for legacy WebViews that don't support cascade layers and would otherwise
/// ignore the full contents of each `@layer` at-rule.
pub fn flatten_css_layers(source: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(source.len());
    let mut index = 0usize;
    let mut brace_depth: u32 = 0;
    let mut skip_closing_brace_at_depth: Vec<u32> = Vec::new();

    let mut state = ScanState::Normal;
    let mut escape = false;

    while index < source.len() {
        let byte = source[index];
        match state {
            ScanState::Comment => {
                if byte == b'*' && index + 1 < source.len() && source[index + 1] == b'/' {
                    output.push(byte);
                    output.push(b'/');
                    index += 2;
                    state = ScanState::Normal;
                    continue;
                }

                output.push(byte);
                index += 1;
            }
            ScanState::SingleQuote => {
                output.push(byte);

                if escape {
                    escape = false;
                    index += 1;
                    continue;
                }

                if byte == b'\\' {
                    escape = true;
                    index += 1;
                    continue;
                }

                if byte == b'\'' {
                    state = ScanState::Normal;
                }
                index += 1;
            }
            ScanState::DoubleQuote => {
                output.push(byte);

                if escape {
                    escape = false;
                    index += 1;
                    continue;
                }

                if byte == b'\\' {
                    escape = true;
                    index += 1;
                    continue;
                }

                if byte == b'"' {
                    state = ScanState::Normal;
                }
                index += 1;
            }
            ScanState::Normal => {
                if byte == b'/' && index + 1 < source.len() && source[index + 1] == b'*' {
                    output.push(byte);
                    output.push(b'*');
                    index += 2;
                    state = ScanState::Comment;
                    continue;
                }

                if byte == b'\'' {
                    output.push(byte);
                    index += 1;
                    state = ScanState::SingleQuote;
                    escape = false;
                    continue;
                }

                if byte == b'"' {
                    output.push(byte);
                    index += 1;
                    state = ScanState::DoubleQuote;
                    escape = false;
                    continue;
                }

                if byte == b'@' {
                    if let Some(end) = try_parse_layer_at_rule(source, index) {
                        match end {
                            LayerAtRuleEnd::StatementEnd(end_index) => {
                                index = end_index + 1;
                                continue;
                            }
                            LayerAtRuleEnd::BlockStart(block_index) => {
                                brace_depth = brace_depth.saturating_add(1);
                                skip_closing_brace_at_depth.push(brace_depth);
                                index = block_index + 1;
                                continue;
                            }
                        }
                    }
                }

                match byte {
                    b'{' => {
                        brace_depth = brace_depth.saturating_add(1);
                        output.push(byte);
                    }
                    b'}' => {
                        if skip_closing_brace_at_depth.last() == Some(&brace_depth) {
                            skip_closing_brace_at_depth.pop();
                        } else {
                            output.push(byte);
                        }
                        brace_depth = brace_depth.saturating_sub(1);
                    }
                    _ => output.push(byte),
                }

                index += 1;
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_layer_statements() {
        let input = br#"@layer base;.x{color:red;}"#;
        let output = flatten_css_layers(input);
        assert_eq!(output, br#".x{color:red;}"#);
    }

    #[test]
    fn flattens_layer_blocks() {
        let input = br#"@layer base{.x{color:red;}}"#;
        let output = flatten_css_layers(input);
        assert_eq!(output, br#".x{color:red;}"#);
    }

    #[test]
    fn flattens_nested_layer_blocks() {
        let input = br#"@layer a{@layer b{.x{color:red;}}}"#;
        let output = flatten_css_layers(input);
        assert_eq!(output, br#".x{color:red;}"#);
    }

    #[test]
    fn ignores_layer_like_sequences_in_strings() {
        let input = br#".x{content:"@layer a{"}@layer a{.y{}}"#;
        let output = flatten_css_layers(input);
        assert_eq!(output, br#".x{content:"@layer a{"}.y{}"#);
    }

    #[test]
    fn ignores_layer_like_sequences_in_comments() {
        let input = br#"/*@layer a{.z{}}*/@layer a{.y{}}"#;
        let output = flatten_css_layers(input);
        assert_eq!(output, br#"/*@layer a{.z{}}*/.y{}"#);
    }

    #[test]
    fn detects_layer_keyword_case_insensitive() {
        assert!(contains_layer_keyword(br#"@LAYER a{.x{}}"#));
        assert!(!contains_layer_keyword(br#".x{color:red;}"#));
    }
}
