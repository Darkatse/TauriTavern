#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCompletionType {
    Ooba,
    Vllm,
    Aphrodite,
    Tabby,
    KoboldCpp,
    LlamaCpp,
    Ollama,
    HuggingFace,
    Generic,
    Featherless,
    OpenRouter,
    InfermaticAi,
    DreamGen,
    TogetherAi,
}

impl TextCompletionType {
    pub const ALL: &'static [Self] = &[
        Self::Ooba,
        Self::Vllm,
        Self::Aphrodite,
        Self::Tabby,
        Self::KoboldCpp,
        Self::LlamaCpp,
        Self::Ollama,
        Self::HuggingFace,
        Self::Generic,
        Self::Featherless,
        Self::OpenRouter,
        Self::InfermaticAi,
        Self::DreamGen,
        Self::TogetherAi,
    ];

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "ooba" => Some(Self::Ooba),
            "vllm" => Some(Self::Vllm),
            "aphrodite" => Some(Self::Aphrodite),
            "tabby" => Some(Self::Tabby),
            "koboldcpp" => Some(Self::KoboldCpp),
            "llamacpp" => Some(Self::LlamaCpp),
            "ollama" => Some(Self::Ollama),
            "huggingface" => Some(Self::HuggingFace),
            "generic" => Some(Self::Generic),
            "featherless" => Some(Self::Featherless),
            "openrouter" => Some(Self::OpenRouter),
            "infermaticai" => Some(Self::InfermaticAi),
            "dreamgen" => Some(Self::DreamGen),
            "togetherai" => Some(Self::TogetherAi),
            _ => None,
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::Ooba => "ooba",
            Self::Vllm => "vllm",
            Self::Aphrodite => "aphrodite",
            Self::Tabby => "tabby",
            Self::KoboldCpp => "koboldcpp",
            Self::LlamaCpp => "llamacpp",
            Self::Ollama => "ollama",
            Self::HuggingFace => "huggingface",
            Self::Generic => "generic",
            Self::Featherless => "featherless",
            Self::OpenRouter => "openrouter",
            Self::InfermaticAi => "infermaticai",
            Self::DreamGen => "dreamgen",
            Self::TogetherAi => "togetherai",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Ooba => "Text Generation WebUI",
            Self::Vllm => "vLLM",
            Self::Aphrodite => "Aphrodite",
            Self::Tabby => "TabbyAPI",
            Self::KoboldCpp => "KoboldCpp",
            Self::LlamaCpp => "llama.cpp",
            Self::Ollama => "Ollama",
            Self::HuggingFace => "HuggingFace",
            Self::Generic => "Generic text completion",
            Self::Featherless => "Featherless",
            Self::OpenRouter => "OpenRouter",
            Self::InfermaticAi => "InfermaticAI",
            Self::DreamGen => "DreamGen",
            Self::TogetherAi => "Together AI",
        }
    }
}
