/// Text completion backends TauriTavern implements, each at an address the user configures.
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
}

impl TextCompletionType {
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
        }
    }
}
