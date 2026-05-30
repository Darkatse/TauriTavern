use serde::Serialize;

const INFERENCE_PROFILE_PREFIXES: &[&str] = &["us.", "eu.", "apac.", "global.", "us-gov."];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BedrockModelFamily {
    AnthropicClaude,
    AmazonNova,
    MetaLlama,
    MistralTextCompletion,
    MistralChat,
    DeepSeekTextCompletion,
    DeepSeekChat,
    CohereCommandR,
    Ai21Jamba,
    Unsupported,
}

impl BedrockModelFamily {
    pub const fn key(self) -> &'static str {
        match self {
            Self::AnthropicClaude => "anthropic_claude",
            Self::AmazonNova => "amazon_nova",
            Self::MetaLlama => "meta_llama",
            Self::MistralTextCompletion => "mistral_text_completion",
            Self::MistralChat => "mistral_chat",
            Self::DeepSeekTextCompletion => "deepseek_text_completion",
            Self::DeepSeekChat => "deepseek_chat",
            Self::CohereCommandR => "cohere_command_r",
            Self::Ai21Jamba => "ai21_jamba",
            Self::Unsupported => "unsupported",
        }
    }

    pub const fn is_supported(self) -> bool {
        !matches!(self, Self::Unsupported)
    }

    pub const fn capabilities(self) -> BedrockModelCapabilities {
        match self {
            Self::AnthropicClaude => BedrockModelCapabilities {
                stream: true,
                tools: true,
                images: true,
                reasoning: true,
                web_search: false,
            },
            Self::MistralChat => BedrockModelCapabilities {
                stream: true,
                tools: true,
                images: false,
                reasoning: false,
                web_search: false,
            },
            Self::AmazonNova
            | Self::MetaLlama
            | Self::MistralTextCompletion
            | Self::DeepSeekTextCompletion
            | Self::DeepSeekChat
            | Self::CohereCommandR
            | Self::Ai21Jamba => BedrockModelCapabilities {
                stream: true,
                tools: false,
                images: false,
                reasoning: false,
                web_search: false,
            },
            Self::Unsupported => BedrockModelCapabilities {
                stream: false,
                tools: false,
                images: false,
                reasoning: false,
                web_search: false,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BedrockModelCapabilities {
    pub stream: bool,
    pub tools: bool,
    pub images: bool,
    pub reasoning: bool,
    pub web_search: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BedrockModelSpec {
    raw_id: String,
    provider: String,
    normalized_id: String,
    family: BedrockModelFamily,
}

impl BedrockModelSpec {
    pub fn classify(raw_id: &str) -> Self {
        let raw_id = raw_id.trim().to_string();
        let normalized_id = strip_inference_profile_prefix(&raw_id).to_string();
        let normalized_lower = normalized_id.to_ascii_lowercase();
        let provider = extract_provider_from_normalized(&normalized_id)
            .trim()
            .to_ascii_lowercase();

        let family = match provider.as_str() {
            "anthropic" if normalized_lower.starts_with("anthropic.claude") => {
                BedrockModelFamily::AnthropicClaude
            }
            "amazon" if normalized_lower.starts_with("amazon.nova") => {
                BedrockModelFamily::AmazonNova
            }
            "meta" if normalized_lower.contains("llama") => BedrockModelFamily::MetaLlama,
            "mistral" if is_mistral_text_completion_id(&normalized_lower) => {
                BedrockModelFamily::MistralTextCompletion
            }
            "mistral" => BedrockModelFamily::MistralChat,
            "deepseek" if is_deepseek_text_completion_id(&normalized_lower) => {
                BedrockModelFamily::DeepSeekTextCompletion
            }
            "deepseek" => BedrockModelFamily::DeepSeekChat,
            "cohere" if normalized_lower.starts_with("cohere.command-r") => {
                BedrockModelFamily::CohereCommandR
            }
            "ai21" if normalized_lower.contains(".jamba") => BedrockModelFamily::Ai21Jamba,
            _ => BedrockModelFamily::Unsupported,
        };

        Self {
            raw_id,
            provider,
            normalized_id,
            family,
        }
    }

    pub fn raw_id(&self) -> &str {
        &self.raw_id
    }

    pub fn provider(&self) -> &str {
        if self.provider.is_empty() {
            "unknown"
        } else {
            &self.provider
        }
    }

    pub fn normalized_id(&self) -> &str {
        &self.normalized_id
    }

    pub fn family(&self) -> BedrockModelFamily {
        self.family
    }

    pub fn family_key(&self) -> &'static str {
        self.family.key()
    }

    pub fn is_supported(&self) -> bool {
        self.family.is_supported()
    }

    pub fn capabilities(&self) -> BedrockModelCapabilities {
        self.family.capabilities()
    }

    pub fn unsupported_reason(&self) -> Option<&'static str> {
        if self.is_supported() {
            return None;
        }
        match self.provider() {
            "amazon" => Some(
                "Only Amazon Nova chat models are wired; Titan and other Amazon model families require a dedicated payload builder or a custom Bedrock template.",
            ),
            "cohere" => Some(
                "Only Cohere Command R/R+ chat models are wired; legacy Cohere text models require a dedicated payload builder or a custom Bedrock template.",
            ),
            "ai21" => Some(
                "Only AI21 Jamba chat models are wired; Jurassic models require a dedicated payload builder or a custom Bedrock template.",
            ),
            "unknown" => Some("The Bedrock model id does not expose a provider segment."),
            _ => Some("This Bedrock provider is not wired by TauriTavern's built-in adapter yet."),
        }
    }
}

pub fn strip_inference_profile_prefix(id: &str) -> &str {
    let id = id.trim();
    INFERENCE_PROFILE_PREFIXES
        .iter()
        .find_map(|prefix| id.strip_prefix(prefix))
        .unwrap_or(id)
}

pub fn extract_provider(id: &str) -> &str {
    extract_provider_from_normalized(strip_inference_profile_prefix(id))
}

pub fn is_mistral_text_completion_model(id: &str) -> bool {
    is_mistral_text_completion_id(&strip_inference_profile_prefix(id).to_ascii_lowercase())
}

pub fn is_deepseek_text_completion_model(id: &str) -> bool {
    is_deepseek_text_completion_id(&strip_inference_profile_prefix(id).to_ascii_lowercase())
}

fn extract_provider_from_normalized(id: &str) -> &str {
    id.split('.').next().unwrap_or(id)
}

fn is_mistral_text_completion_id(lower_id: &str) -> bool {
    lower_id.contains("mistral-7b") || lower_id.contains("mixtral") || lower_id.contains("-2402")
}

fn is_deepseek_text_completion_id(lower_id: &str) -> bool {
    lower_id.contains("r1")
}

#[cfg(test)]
mod tests {
    use super::{BedrockModelFamily, BedrockModelSpec, extract_provider};

    #[test]
    fn strips_inference_profile_prefix_before_classification() {
        let spec = BedrockModelSpec::classify("us.anthropic.claude-opus-4-7");

        assert_eq!(spec.provider(), "anthropic");
        assert_eq!(spec.normalized_id(), "anthropic.claude-opus-4-7");
        assert_eq!(spec.family(), BedrockModelFamily::AnthropicClaude);
    }

    #[test]
    fn classifies_wired_bedrock_model_families() {
        let cases = [
            (
                "anthropic.claude-sonnet-4-20250514-v1:0",
                BedrockModelFamily::AnthropicClaude,
            ),
            ("us.amazon.nova-pro-v1:0", BedrockModelFamily::AmazonNova),
            (
                "meta.llama3-70b-instruct-v1:0",
                BedrockModelFamily::MetaLlama,
            ),
            (
                "mistral.mistral-7b-instruct-v0:2",
                BedrockModelFamily::MistralTextCompletion,
            ),
            (
                "mistral.mistral-large-2407-v1:0",
                BedrockModelFamily::MistralChat,
            ),
            (
                "us.deepseek.r1-v1:0",
                BedrockModelFamily::DeepSeekTextCompletion,
            ),
            ("deepseek.v3-v1:0", BedrockModelFamily::DeepSeekChat),
            (
                "cohere.command-r-plus-v1:0",
                BedrockModelFamily::CohereCommandR,
            ),
            (
                "us.ai21.jamba-1-5-large-v1:0",
                BedrockModelFamily::Ai21Jamba,
            ),
        ];

        for (id, expected) in cases {
            let spec = BedrockModelSpec::classify(id);
            assert_eq!(spec.family(), expected, "wrong family for {id}");
            assert!(spec.is_supported(), "{id} should be supported");
        }
    }

    #[test]
    fn classifies_known_provider_legacy_families_as_unsupported() {
        for id in [
            "amazon.titan-text-premier-v1:0",
            "cohere.command-text-v14",
            "ai21.j2-ultra-v1",
            "stability.stable-diffusion-xl-v1",
        ] {
            let spec = BedrockModelSpec::classify(id);
            assert_eq!(spec.family(), BedrockModelFamily::Unsupported, "{id}");
            assert!(!spec.is_supported(), "{id}");
            assert!(spec.unsupported_reason().is_some(), "{id}");
        }
    }

    #[test]
    fn exposes_conservative_capabilities_by_family() {
        let claude = BedrockModelSpec::classify("anthropic.claude-sonnet-4-20250514-v1:0");
        assert!(claude.capabilities().stream);
        assert!(claude.capabilities().tools);
        assert!(claude.capabilities().images);
        assert!(claude.capabilities().reasoning);
        assert!(!claude.capabilities().web_search);

        let nova = BedrockModelSpec::classify("amazon.nova-pro-v1:0");
        assert!(nova.capabilities().stream);
        assert!(!nova.capabilities().tools);
        assert!(!nova.capabilities().images);
    }

    #[test]
    fn extract_provider_strips_inference_profile_prefix() {
        assert_eq!(extract_provider("anthropic.claude-3-haiku"), "anthropic");
        assert_eq!(
            extract_provider("us.anthropic.claude-opus-4-7"),
            "anthropic"
        );
        assert_eq!(extract_provider("amazon.nova-pro-v1:0"), "amazon");
        assert_eq!(extract_provider("global.deepseek.r1-v1:0"), "deepseek");
    }
}
