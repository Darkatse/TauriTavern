use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 表示用户的API密钥和其他敏感信息
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Secrets {
    /// 所有密钥的映射，键为密钥名称，值为密钥值
    #[serde(flatten)]
    pub secrets: HashMap<String, String>,
}

impl Secrets {
    /// 创建一个新的空Secrets实例
    pub fn new() -> Self {
        Self {
            secrets: HashMap::new(),
        }
    }

    /// 获取指定密钥的值
    pub fn get(&self, key: &str) -> Option<&String> {
        self.secrets.get(key)
    }

    /// 设置指定密钥的值
    pub fn set(&mut self, key: String, value: String) {
        self.secrets.insert(key, value);
    }

    /// 删除指定密钥
    pub fn delete(&mut self, key: &str) -> Option<String> {
        self.secrets.remove(key)
    }

    /// 检查指定密钥是否存在
    pub fn has_key(&self, key: &str) -> bool {
        self.secrets.contains_key(key)
    }

    /// 获取所有密钥的状态（是否存在有效值）
    pub fn get_state(&self) -> HashMap<String, bool> {
        let mut state = HashMap::new();
        for (key, value) in &self.secrets {
            state.insert(key.clone(), !value.is_empty());
        }
        state
    }
}

/// 定义常用的密钥名称
pub struct SecretKeys;

impl SecretKeys {
    pub const HORDE: &'static str = "api_key_horde";
    pub const MANCER: &'static str = "api_key_mancer";
    pub const VLLM: &'static str = "api_key_vllm";
    pub const APHRODITE: &'static str = "api_key_aphrodite";
    pub const TABBY: &'static str = "api_key_tabby";
    pub const OPENAI: &'static str = "api_key_openai";
    pub const NOVEL: &'static str = "api_key_novel";
    pub const CLAUDE: &'static str = "api_key_claude";
    pub const OPENROUTER: &'static str = "api_key_openrouter";
    pub const SCALE: &'static str = "api_key_scale";
    pub const AI21: &'static str = "api_key_ai21";
    pub const SCALE_COOKIE: &'static str = "scale_cookie";
    pub const MAKERSUITE: &'static str = "api_key_makersuite";
    pub const SERPAPI: &'static str = "api_key_serpapi";
    pub const MISTRALAI: &'static str = "api_key_mistralai";
    pub const TOGETHERAI: &'static str = "api_key_togetherai";
    pub const INFERMATICAI: &'static str = "api_key_infermaticai";
    pub const DREAMGEN: &'static str = "api_key_dreamgen";
    pub const CUSTOM: &'static str = "api_key_custom";
    pub const OOBA: &'static str = "api_key_ooba";
    pub const NOMICAI: &'static str = "api_key_nomicai";
    pub const KOBOLDCPP: &'static str = "api_key_koboldcpp";
    pub const LLAMACPP: &'static str = "api_key_llamacpp";
    pub const COHERE: &'static str = "api_key_cohere";
    pub const PERPLEXITY: &'static str = "api_key_perplexity";
    pub const GROQ: &'static str = "api_key_groq";
    pub const AZURE_TTS: &'static str = "api_key_azure_tts";
    pub const FEATHERLESS: &'static str = "api_key_featherless";
    pub const ZEROONEAI: &'static str = "api_key_01ai";
    pub const HUGGINGFACE: &'static str = "api_key_huggingface";
    pub const STABILITY: &'static str = "api_key_stability";
    pub const CUSTOM_OPENAI_TTS: &'static str = "api_key_custom_openai_tts";
    pub const NANOGPT: &'static str = "api_key_nanogpt";
    pub const TAVILY: &'static str = "api_key_tavily";
    pub const BFL: &'static str = "api_key_bfl";
    pub const GENERIC: &'static str = "api_key_generic";
    pub const DEEPSEEK: &'static str = "api_key_deepseek";
    pub const MOONSHOT: &'static str = "api_key_moonshot";
    pub const SILICONFLOW: &'static str = "api_key_siliconflow";
    pub const ZAI: &'static str = "api_key_zai";
    pub const SERPER: &'static str = "api_key_serper";
    pub const FALAI: &'static str = "api_key_falai";
    pub const XAI: &'static str = "api_key_xai";
    pub const CSRF_SECRET: &'static str = "csrfSecret";

    /// 获取可以安全暴露的密钥列表（即使allowKeysExposure为false）
    pub fn get_exportable_keys() -> Vec<&'static str> {
        vec![
            "libre_url",
            "lingva_url",
            "oneringtranslator_url",
            "deeplx_url",
        ]
    }
}
