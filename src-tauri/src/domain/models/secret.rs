use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretEntry {
    pub id: String,
    pub value: String,
    pub label: String,
    pub active: bool,
}

impl SecretEntry {
    pub fn new(value: String, label: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            value,
            label,
            active: true,
        }
    }
}

/// 表示用户的 API 密钥集合，结构与 SillyTavern 1.15.0 的 secrets.json 一致：
/// `{ "api_key_xxx": [{ id, value, label, active }], ... }`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Secrets {
    #[serde(flatten)]
    pub secrets: HashMap<String, Vec<SecretEntry>>,
}

impl Secrets {
    pub fn new() -> Self {
        Self {
            secrets: HashMap::new(),
        }
    }

    pub fn write_secret(&mut self, key: String, value: String, label: String) -> String {
        let entries = self.secrets.entry(key).or_default();
        for entry in entries.iter_mut() {
            entry.active = false;
        }

        let entry = SecretEntry::new(value, label);
        let id = entry.id.clone();
        entries.push(entry);
        id
    }

    pub fn read_secret(&self, key: &str, id: Option<&str>) -> Option<String> {
        self.secrets
            .get(key)
            .and_then(|entries| Self::find_entry(entries, id))
            .map(|entry| entry.value.clone())
    }

    pub fn delete_secret(&mut self, key: &str, id: Option<&str>) -> bool {
        let Some(entries) = self.secrets.get_mut(key) else {
            return false;
        };

        let target_index = match id {
            Some(secret_id) => entries.iter().position(|entry| entry.id == secret_id),
            None => entries.iter().position(|entry| entry.active),
        };

        let Some(target_index) = target_index else {
            return false;
        };

        entries.remove(target_index);

        if entries.is_empty() {
            self.secrets.remove(key);
        } else {
            Self::normalize_active(entries);
        }

        true
    }

    pub fn rotate_secret(&mut self, key: &str, id: &str) -> bool {
        let Some(entries) = self.secrets.get_mut(key) else {
            return false;
        };

        let Some(target_index) = entries.iter().position(|entry| entry.id == id) else {
            return false;
        };

        for entry in entries.iter_mut() {
            entry.active = false;
        }
        entries[target_index].active = true;
        true
    }

    pub fn rename_secret(&mut self, key: &str, id: &str, label: String) -> bool {
        let Some(entries) = self.secrets.get_mut(key) else {
            return false;
        };

        let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };

        entry.label = label;
        true
    }

    pub fn active_secret_values(&self) -> HashMap<String, String> {
        let mut result = HashMap::new();

        for (key, entries) in &self.secrets {
            if key == SecretKeys::MIGRATED {
                continue;
            }

            if let Some(active) = entries.iter().find(|entry| entry.active) {
                result.insert(key.clone(), active.value.clone());
            }
        }

        result
    }

    pub fn normalize_entries(entries: &mut Vec<SecretEntry>) {
        entries.retain(|entry| !entry.id.trim().is_empty());
        Self::normalize_active(entries);
    }

    fn find_entry<'a>(entries: &'a [SecretEntry], id: Option<&str>) -> Option<&'a SecretEntry> {
        match id {
            Some(secret_id) => entries.iter().find(|entry| entry.id == secret_id),
            None => entries.iter().find(|entry| entry.active),
        }
    }

    fn normalize_active(entries: &mut [SecretEntry]) {
        if entries.is_empty() {
            return;
        }

        let mut first_active = None;
        for (index, entry) in entries.iter_mut().enumerate() {
            if entry.active {
                if first_active.is_none() {
                    first_active = Some(index);
                } else {
                    entry.active = false;
                }
            }
        }

        if first_active.is_none() {
            entries[0].active = true;
        }
    }
}

/// 定义常用密钥名称（与 SillyTavern 1.15.0 对齐）
pub struct SecretKeys;

impl SecretKeys {
    pub const MIGRATED: &'static str = "_migrated";
    pub const HORDE: &'static str = "api_key_horde";
    pub const MANCER: &'static str = "api_key_mancer";
    pub const VLLM: &'static str = "api_key_vllm";
    pub const APHRODITE: &'static str = "api_key_aphrodite";
    pub const TABBY: &'static str = "api_key_tabby";
    pub const OPENAI: &'static str = "api_key_openai";
    pub const NOVEL: &'static str = "api_key_novel";
    pub const CLAUDE: &'static str = "api_key_claude";
    pub const DEEPL: &'static str = "deepl";
    pub const LIBRE: &'static str = "libre";
    pub const LIBRE_URL: &'static str = "libre_url";
    pub const LINGVA_URL: &'static str = "lingva_url";
    pub const OPENROUTER: &'static str = "api_key_openrouter";
    pub const AI21: &'static str = "api_key_ai21";
    pub const ONERING_URL: &'static str = "oneringtranslator_url";
    pub const DEEPLX_URL: &'static str = "deeplx_url";
    pub const MAKERSUITE: &'static str = "api_key_makersuite";
    pub const VERTEXAI: &'static str = "api_key_vertexai";
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
    pub const AZURE_OPENAI: &'static str = "api_key_azure_openai";
    pub const FEATHERLESS: &'static str = "api_key_featherless";
    pub const HUGGINGFACE: &'static str = "api_key_huggingface";
    pub const STABILITY: &'static str = "api_key_stability";
    pub const CUSTOM_OPENAI_TTS: &'static str = "api_key_custom_openai_tts";
    pub const CHUTES: &'static str = "api_key_chutes";
    pub const ELECTRONHUB: &'static str = "api_key_electronhub";
    pub const NANOGPT: &'static str = "api_key_nanogpt";
    pub const TAVILY: &'static str = "api_key_tavily";
    pub const BFL: &'static str = "api_key_bfl";
    pub const COMFY_RUNPOD: &'static str = "api_key_comfy_runpod";
    pub const GENERIC: &'static str = "api_key_generic";
    pub const DEEPSEEK: &'static str = "api_key_deepseek";
    pub const SERPER: &'static str = "api_key_serper";
    pub const AIMLAPI: &'static str = "api_key_aimlapi";
    pub const FALAI: &'static str = "api_key_falai";
    pub const XAI: &'static str = "api_key_xai";
    pub const FIREWORKS: &'static str = "api_key_fireworks";
    pub const VERTEXAI_SERVICE_ACCOUNT: &'static str = "vertexai_service_account_json";
    pub const MINIMAX: &'static str = "api_key_minimax";
    pub const MINIMAX_GROUP_ID: &'static str = "minimax_group_id";
    pub const MOONSHOT: &'static str = "api_key_moonshot";
    pub const COMETAPI: &'static str = "api_key_cometapi";
    pub const ZAI: &'static str = "api_key_zai";
    pub const SILICONFLOW: &'static str = "api_key_siliconflow";
    pub const ELEVENLABS: &'static str = "api_key_elevenlabs";
    pub const POLLINATIONS: &'static str = "api_key_pollinations";
    pub const VOLCENGINE_APP_ID: &'static str = "volcengine_app_id";
    pub const VOLCENGINE_ACCESS_KEY: &'static str = "volcengine_access_key";

    pub fn known_keys() -> &'static [&'static str] {
        &[
            Self::HORDE,
            Self::MANCER,
            Self::VLLM,
            Self::APHRODITE,
            Self::TABBY,
            Self::OPENAI,
            Self::NOVEL,
            Self::CLAUDE,
            Self::DEEPL,
            Self::LIBRE,
            Self::LIBRE_URL,
            Self::LINGVA_URL,
            Self::OPENROUTER,
            Self::AI21,
            Self::ONERING_URL,
            Self::DEEPLX_URL,
            Self::MAKERSUITE,
            Self::VERTEXAI,
            Self::SERPAPI,
            Self::MISTRALAI,
            Self::TOGETHERAI,
            Self::INFERMATICAI,
            Self::DREAMGEN,
            Self::CUSTOM,
            Self::OOBA,
            Self::NOMICAI,
            Self::KOBOLDCPP,
            Self::LLAMACPP,
            Self::COHERE,
            Self::PERPLEXITY,
            Self::GROQ,
            Self::AZURE_TTS,
            Self::AZURE_OPENAI,
            Self::FEATHERLESS,
            Self::HUGGINGFACE,
            Self::STABILITY,
            Self::CUSTOM_OPENAI_TTS,
            Self::CHUTES,
            Self::ELECTRONHUB,
            Self::NANOGPT,
            Self::TAVILY,
            Self::BFL,
            Self::COMFY_RUNPOD,
            Self::GENERIC,
            Self::DEEPSEEK,
            Self::SERPER,
            Self::AIMLAPI,
            Self::FALAI,
            Self::XAI,
            Self::FIREWORKS,
            Self::VERTEXAI_SERVICE_ACCOUNT,
            Self::MINIMAX,
            Self::MINIMAX_GROUP_ID,
            Self::MOONSHOT,
            Self::COMETAPI,
            Self::ZAI,
            Self::SILICONFLOW,
            Self::ELEVENLABS,
            Self::POLLINATIONS,
            Self::VOLCENGINE_APP_ID,
            Self::VOLCENGINE_ACCESS_KEY,
        ]
    }

    /// 即使 `allowKeysExposure` 为 false 也可导出的密钥
    pub fn get_exportable_keys() -> &'static [&'static str] {
        &[
            Self::LIBRE_URL,
            Self::LINGVA_URL,
            Self::ONERING_URL,
            Self::DEEPLX_URL,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::Secrets;

    #[test]
    fn write_secret_sets_latest_active() {
        let mut secrets = Secrets::new();
        let first_id = secrets.write_secret(
            "api_key_openai".to_string(),
            "first".to_string(),
            "1".to_string(),
        );
        let second_id = secrets.write_secret(
            "api_key_openai".to_string(),
            "second".to_string(),
            "2".to_string(),
        );

        let entries = secrets.secrets.get("api_key_openai").unwrap();
        assert_eq!(entries.len(), 2);
        assert!(
            !entries
                .iter()
                .find(|entry| entry.id == first_id)
                .unwrap()
                .active
        );
        assert!(
            entries
                .iter()
                .find(|entry| entry.id == second_id)
                .unwrap()
                .active
        );
        assert_eq!(
            secrets.read_secret("api_key_openai", None),
            Some("second".to_string())
        );
    }

    #[test]
    fn rotate_delete_and_rename_secret() {
        let mut secrets = Secrets::new();
        let first_id = secrets.write_secret(
            "api_key_openai".to_string(),
            "first".to_string(),
            "1".to_string(),
        );
        let second_id = secrets.write_secret(
            "api_key_openai".to_string(),
            "second".to_string(),
            "2".to_string(),
        );

        assert!(secrets.rotate_secret("api_key_openai", &first_id));
        assert_eq!(
            secrets.read_secret("api_key_openai", None),
            Some("first".to_string())
        );

        assert!(secrets.rename_secret("api_key_openai", &first_id, "renamed".to_string()));
        assert_eq!(
            secrets
                .secrets
                .get("api_key_openai")
                .unwrap()
                .iter()
                .find(|entry| entry.id == first_id)
                .unwrap()
                .label,
            "renamed"
        );

        assert!(secrets.delete_secret("api_key_openai", Some(&first_id)));
        assert_eq!(
            secrets.read_secret("api_key_openai", None),
            Some("second".to_string())
        );
        assert!(secrets.delete_secret("api_key_openai", Some(&second_id)));
        assert!(!secrets.secrets.contains_key("api_key_openai"));
    }
}
