use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMetrics {
    pub chars: usize,
    pub words: usize,
}

impl TextMetrics {
    pub fn from_text(text: &str) -> Self {
        let mut chars = 0_usize;
        let mut words = 0_usize;
        let mut in_word = false;

        for character in text.chars() {
            chars += 1;
            if is_cjk_word_unit(character) {
                words += 1;
                in_word = false;
            } else if character.is_alphanumeric() {
                if !in_word {
                    words += 1;
                    in_word = true;
                }
            } else {
                in_word = false;
            }
        }

        Self { chars, words }
    }
}

fn is_cjk_word_unit(character: char) -> bool {
    matches!(
        character as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0x2CEB0..=0x2EBEF
            | 0x30000..=0x3134F
            | 0x3040..=0x309F
            | 0x30A0..=0x30FF
            | 0xFF66..=0xFF9F
            | 0xAC00..=0xD7AF
            | 0x1100..=0x11FF
            | 0x3130..=0x318F
    )
}

#[cfg(test)]
mod tests {
    use super::TextMetrics;

    #[test]
    fn counts_ascii_words_and_chars() {
        assert_eq!(
            TextMetrics::from_text("Hello, brave new world!"),
            TextMetrics {
                chars: 23,
                words: 4,
            }
        );
    }

    #[test]
    fn counts_cjk_characters_as_word_units() {
        assert_eq!(
            TextMetrics::from_text("你好，世界"),
            TextMetrics { chars: 5, words: 4 }
        );
    }

    #[test]
    fn counts_mixed_script_text() {
        assert_eq!(
            TextMetrics::from_text("Chapter 2：你好world"),
            TextMetrics {
                chars: 17,
                words: 5,
            }
        );
    }

    #[test]
    fn ignores_punctuation_and_emoji_for_words() {
        assert_eq!(
            TextMetrics::from_text("✨ ..."),
            TextMetrics { chars: 5, words: 0 }
        );
    }
}
