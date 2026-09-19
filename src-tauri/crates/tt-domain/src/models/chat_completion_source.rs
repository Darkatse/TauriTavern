#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatCompletionSource {
    OpenAi,
    OpenCode,
    OpenRouter,
    Custom,
    Claude,
    Makersuite,
    VertexAi,
    DeepSeek,
    Cohere,
    Groq,
    Moonshot,
    NanoGpt,
    Chutes,
    SiliconFlow,
    WorkersAi,
    Zai,
    MiniMax,
    AwsBedrock,
    Xai,
    Pollinations,
}

impl ChatCompletionSource {
    pub const ALL: &'static [Self] = &[
        Self::OpenAi,
        Self::OpenCode,
        Self::OpenRouter,
        Self::Custom,
        Self::Claude,
        Self::Makersuite,
        Self::VertexAi,
        Self::DeepSeek,
        Self::Cohere,
        Self::Groq,
        Self::Moonshot,
        Self::NanoGpt,
        Self::Chutes,
        Self::SiliconFlow,
        Self::WorkersAi,
        Self::Zai,
        Self::MiniMax,
        Self::AwsBedrock,
        Self::Xai,
        Self::Pollinations,
    ];

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_lowercase().as_str() {
            "" | "openai" => Some(Self::OpenAi),
            "opencode" => Some(Self::OpenCode),
            "openrouter" | "open-router" => Some(Self::OpenRouter),
            "custom" => Some(Self::Custom),
            "claude" => Some(Self::Claude),
            "makersuite" | "gemini" | "google" => Some(Self::Makersuite),
            "vertexai" | "vertex-ai" | "vertex ai" => Some(Self::VertexAi),
            "deepseek" => Some(Self::DeepSeek),
            "cohere" => Some(Self::Cohere),
            "groq" => Some(Self::Groq),
            "moonshot" | "moonshot ai" => Some(Self::Moonshot),
            "nanogpt" | "nano-gpt" | "nano gpt" => Some(Self::NanoGpt),
            "chutes" => Some(Self::Chutes),
            "siliconflow" | "silicon flow" => Some(Self::SiliconFlow),
            "workers_ai" | "workers-ai" | "workers ai" | "cloudflare workers ai" => {
                Some(Self::WorkersAi)
            }
            "zai" | "z.ai" | "glm" => Some(Self::Zai),
            "minimax" | "mini-max" | "mini max" => Some(Self::MiniMax),
            "aws_bedrock" | "aws-bedrock" | "aws bedrock" | "bedrock" => Some(Self::AwsBedrock),
            "xai" | "x.ai" | "grok" => Some(Self::Xai),
            "pollinations" => Some(Self::Pollinations),
            _ => None,
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::OpenCode => "opencode",
            Self::OpenRouter => "openrouter",
            Self::Custom => "custom",
            Self::Claude => "claude",
            Self::Makersuite => "makersuite",
            Self::VertexAi => "vertexai",
            Self::DeepSeek => "deepseek",
            Self::Cohere => "cohere",
            Self::Groq => "groq",
            Self::Moonshot => "moonshot",
            Self::NanoGpt => "nanogpt",
            Self::Chutes => "chutes",
            Self::SiliconFlow => "siliconflow",
            Self::WorkersAi => "workers_ai",
            Self::Zai => "zai",
            Self::MiniMax => "minimax",
            Self::AwsBedrock => "aws_bedrock",
            Self::Xai => "xai",
            Self::Pollinations => "pollinations",
        }
    }

    pub const fn prompt_model_setting_key(self) -> &'static str {
        match self {
            Self::OpenAi => "openai_model",
            Self::OpenCode => "opencode_model",
            Self::OpenRouter => "openrouter_model",
            Self::Custom => "custom_model",
            Self::Claude => "claude_model",
            Self::Makersuite => "google_model",
            Self::VertexAi => "vertexai_model",
            Self::DeepSeek => "deepseek_model",
            Self::Cohere => "cohere_model",
            Self::Groq => "groq_model",
            Self::Moonshot => "moonshot_model",
            Self::NanoGpt => "nanogpt_model",
            Self::Chutes => "chutes_model",
            Self::SiliconFlow => "siliconflow_model",
            Self::WorkersAi => "workers_ai_model",
            Self::Zai => "zai_model",
            Self::MiniMax => "minimax_model",
            Self::AwsBedrock => "aws_bedrock_model",
            Self::Xai => "xai_model",
            Self::Pollinations => "pollinations_model",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::OpenCode => "OpenCode",
            Self::OpenRouter => "OpenRouter",
            Self::Custom => "Custom OpenAI",
            Self::Claude => "Claude",
            Self::Makersuite => "Google Gemini",
            Self::VertexAi => "Google Vertex AI",
            Self::DeepSeek => "DeepSeek",
            Self::Cohere => "Cohere",
            Self::Groq => "Groq",
            Self::Moonshot => "Moonshot AI",
            Self::NanoGpt => "NanoGPT",
            Self::Chutes => "Chutes",
            Self::SiliconFlow => "SiliconFlow",
            Self::WorkersAi => "Cloudflare Workers AI",
            Self::Zai => "Z.AI (GLM)",
            Self::MiniMax => "MiniMax",
            Self::AwsBedrock => "AWS Bedrock",
            Self::Xai => "xAI",
            Self::Pollinations => "Pollinations",
        }
    }
}
