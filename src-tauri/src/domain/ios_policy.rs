use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::ChatCompletionSource;

pub const IOS_POLICY_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IosPolicyScope {
    Ios,
    Ignored,
}

impl IosPolicyScope {
    pub fn for_current_platform() -> Self {
        if cfg!(target_os = "ios") {
            Self::Ios
        } else {
            Self::Ignored
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IosPolicyProfile {
    Full,
    IosInternalFull,
    IosExternalBeta,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AllowlistMode {
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum AllowlistSetting {
    Mode(AllowlistMode),
    List(Vec<String>),
}

impl AllowlistSetting {
    pub fn allows(&self, value: &str) -> bool {
        match self {
            Self::Mode(AllowlistMode::All) => true,
            Self::List(items) => items.iter().any(|item| item == value),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct IosPolicyCapabilities {
    pub extensions: IosPolicyExtensionCapabilities,
    pub content: IosPolicyContentCapabilities,
    pub updates: IosPolicyUpdateCapabilities,
    pub prompts: IosPolicyPromptCapabilities,
    pub llm: IosPolicyLlmCapabilities,
    pub network: IosPolicyNetworkCapabilities,
    pub scripting: IosPolicyScriptingCapabilities,
    pub ai: IosPolicyImageGenerationCapabilities,
    pub sync: IosPolicySyncCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct IosPolicyExtensionCapabilities {
    pub third_party_management: bool,
    pub third_party_execution: bool,
    pub system_allowlist: AllowlistSetting,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct IosPolicyContentCapabilities {
    pub external_import: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct IosPolicyUpdateCapabilities {
    pub startup_check: bool,
    pub manual_check: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct IosPolicyPromptCapabilities {
    pub nsfw_prompt: bool,
    pub jailbreak_prompt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct IosPolicyLlmCapabilities {
    pub chat_completion_sources: IosPolicyChatCompletionSourceCapabilities,
    pub chat_completion_features: IosPolicyChatCompletionFeatureCapabilities,
    pub endpoint_overrides: bool,
    pub text_completions: IosPolicyTextCompletionCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct IosPolicyChatCompletionSourceCapabilities {
    pub allowlist: AllowlistSetting,
}

impl IosPolicyChatCompletionSourceCapabilities {
    pub fn allows_source(&self, source: ChatCompletionSource) -> bool {
        match &self.allowlist {
            AllowlistSetting::Mode(AllowlistMode::All) => true,
            AllowlistSetting::List(items) => items
                .iter()
                .any(|raw| ChatCompletionSource::parse(raw).is_some_and(|value| value == source)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct IosPolicyChatCompletionFeatureCapabilities {
    pub web_search: bool,
    pub request_images: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct IosPolicyTextCompletionCapabilities {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct IosPolicyNetworkCapabilities {
    pub request_proxy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct IosPolicyScriptingCapabilities {
    pub prompt_injections: bool,
    pub tool_registration: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct IosPolicyImageGenerationCapabilities {
    pub image_generation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct IosPolicySyncCapabilities {
    pub lan: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct IosPolicyActivationReport {
    pub version: u32,
    pub scope: IosPolicyScope,
    pub profile: IosPolicyProfile,
    pub capabilities: IosPolicyCapabilities,
    pub overridden_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicyConfig {
    pub version: u32,
    pub profile: IosPolicyProfile,
    #[serde(default)]
    pub overrides: IosPolicyOverridesConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicyOverridesConfig {
    #[serde(default)]
    pub capabilities: IosPolicyCapabilitiesOverride,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicyCapabilitiesOverride {
    #[serde(default)]
    pub extensions: IosPolicyExtensionCapabilitiesOverride,
    #[serde(default)]
    pub content: IosPolicyContentCapabilitiesOverride,
    #[serde(default)]
    pub updates: IosPolicyUpdateCapabilitiesOverride,
    #[serde(default)]
    pub prompts: IosPolicyPromptCapabilitiesOverride,
    #[serde(default)]
    pub llm: IosPolicyLlmCapabilitiesOverride,
    #[serde(default)]
    pub network: IosPolicyNetworkCapabilitiesOverride,
    #[serde(default)]
    pub scripting: IosPolicyScriptingCapabilitiesOverride,
    #[serde(default)]
    pub ai: IosPolicyImageGenerationCapabilitiesOverride,
    #[serde(default)]
    pub sync: IosPolicySyncCapabilitiesOverride,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicyExtensionCapabilitiesOverride {
    pub third_party_management: Option<bool>,
    pub third_party_execution: Option<bool>,
    pub system_allowlist: Option<AllowlistSetting>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicyContentCapabilitiesOverride {
    pub external_import: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicyUpdateCapabilitiesOverride {
    pub startup_check: Option<bool>,
    pub manual_check: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicyPromptCapabilitiesOverride {
    pub nsfw_prompt: Option<bool>,
    pub jailbreak_prompt: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicyLlmCapabilitiesOverride {
    #[serde(default)]
    pub chat_completion_sources: IosPolicyChatCompletionSourceCapabilitiesOverride,
    #[serde(default)]
    pub chat_completion_features: IosPolicyChatCompletionFeatureCapabilitiesOverride,
    pub endpoint_overrides: Option<bool>,
    #[serde(default)]
    pub text_completions: IosPolicyTextCompletionCapabilitiesOverride,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicyChatCompletionSourceCapabilitiesOverride {
    pub allowlist: Option<AllowlistSetting>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicyChatCompletionFeatureCapabilitiesOverride {
    pub web_search: Option<bool>,
    pub request_images: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicyTextCompletionCapabilitiesOverride {
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicyNetworkCapabilitiesOverride {
    pub request_proxy: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicyScriptingCapabilitiesOverride {
    pub prompt_injections: Option<bool>,
    pub tool_registration: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicyImageGenerationCapabilitiesOverride {
    pub image_generation: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct IosPolicySyncCapabilitiesOverride {
    pub lan: Option<bool>,
}

impl IosPolicyCapabilities {
    pub fn baseline(profile: IosPolicyProfile) -> Self {
        match profile {
            IosPolicyProfile::Full => Self {
                extensions: IosPolicyExtensionCapabilities {
                    third_party_management: true,
                    third_party_execution: true,
                    system_allowlist: AllowlistSetting::Mode(AllowlistMode::All),
                },
                content: IosPolicyContentCapabilities {
                    external_import: true,
                },
                updates: IosPolicyUpdateCapabilities {
                    startup_check: true,
                    manual_check: true,
                },
                prompts: IosPolicyPromptCapabilities {
                    nsfw_prompt: true,
                    jailbreak_prompt: true,
                },
                llm: IosPolicyLlmCapabilities {
                    chat_completion_sources: IosPolicyChatCompletionSourceCapabilities {
                        allowlist: AllowlistSetting::Mode(AllowlistMode::All),
                    },
                    chat_completion_features: IosPolicyChatCompletionFeatureCapabilities {
                        web_search: true,
                        request_images: true,
                    },
                    endpoint_overrides: true,
                    text_completions: IosPolicyTextCompletionCapabilities { enabled: true },
                },
                network: IosPolicyNetworkCapabilities {
                    request_proxy: true,
                },
                scripting: IosPolicyScriptingCapabilities {
                    prompt_injections: true,
                    tool_registration: true,
                },
                ai: IosPolicyImageGenerationCapabilities {
                    image_generation: true,
                },
                sync: IosPolicySyncCapabilities { lan: true },
            },
            IosPolicyProfile::IosInternalFull => Self {
                updates: IosPolicyUpdateCapabilities {
                    startup_check: false,
                    manual_check: true,
                },
                ..Self::baseline(IosPolicyProfile::Full)
            },
            IosPolicyProfile::IosExternalBeta => Self {
                extensions: IosPolicyExtensionCapabilities {
                    third_party_management: false,
                    third_party_execution: false,
                    system_allowlist: AllowlistSetting::List(vec![
                        "data-migration".to_string(),
                        "regex".to_string(),
                        "quick-reply".to_string(),
                        "tauritavern-version".to_string(),
                        "token-counter".to_string(),
                    ]),
                },
                content: IosPolicyContentCapabilities {
                    external_import: false,
                },
                updates: IosPolicyUpdateCapabilities {
                    startup_check: false,
                    manual_check: false,
                },
                prompts: IosPolicyPromptCapabilities {
                    nsfw_prompt: false,
                    jailbreak_prompt: false,
                },
                llm: IosPolicyLlmCapabilities {
                    chat_completion_sources: IosPolicyChatCompletionSourceCapabilities {
                        allowlist: AllowlistSetting::List(vec![
                            "openai".to_string(),
                            "claude".to_string(),
                            "makersuite".to_string(),
                        ]),
                    },
                    chat_completion_features: IosPolicyChatCompletionFeatureCapabilities {
                        web_search: false,
                        request_images: false,
                    },
                    endpoint_overrides: false,
                    text_completions: IosPolicyTextCompletionCapabilities { enabled: false },
                },
                network: IosPolicyNetworkCapabilities {
                    request_proxy: false,
                },
                scripting: IosPolicyScriptingCapabilities {
                    prompt_injections: false,
                    tool_registration: false,
                },
                ai: IosPolicyImageGenerationCapabilities {
                    image_generation: false,
                },
                sync: IosPolicySyncCapabilities { lan: false },
            },
        }
    }

    fn validate(&self) -> Result<(), DomainError> {
        if let AllowlistSetting::List(items) = &self.llm.chat_completion_sources.allowlist {
            for raw_source in items {
                if ChatCompletionSource::parse(raw_source).is_none() {
                    return Err(DomainError::InvalidData(format!(
                        "ios_policy: invalid llm.chat_completion_sources.allowlist entry: {}",
                        raw_source
                    )));
                }
            }
        }

        Ok(())
    }
}

impl IosPolicyCapabilitiesOverride {
    fn apply(self, capabilities: &mut IosPolicyCapabilities) -> Vec<String> {
        let mut overridden = Vec::new();

        let ext = self.extensions;
        if let Some(value) = ext.third_party_management {
            capabilities.extensions.third_party_management = value;
            overridden.push("extensions.third_party_management".to_string());
        }
        if let Some(value) = ext.third_party_execution {
            capabilities.extensions.third_party_execution = value;
            overridden.push("extensions.third_party_execution".to_string());
        }
        if let Some(value) = ext.system_allowlist {
            capabilities.extensions.system_allowlist = value;
            overridden.push("extensions.system_allowlist".to_string());
        }

        let content = self.content;
        if let Some(value) = content.external_import {
            capabilities.content.external_import = value;
            overridden.push("content.external_import".to_string());
        }

        let updates = self.updates;
        if let Some(value) = updates.startup_check {
            capabilities.updates.startup_check = value;
            overridden.push("updates.startup_check".to_string());
        }
        if let Some(value) = updates.manual_check {
            capabilities.updates.manual_check = value;
            overridden.push("updates.manual_check".to_string());
        }

        let prompts = self.prompts;
        if let Some(value) = prompts.nsfw_prompt {
            capabilities.prompts.nsfw_prompt = value;
            overridden.push("prompts.nsfw_prompt".to_string());
        }
        if let Some(value) = prompts.jailbreak_prompt {
            capabilities.prompts.jailbreak_prompt = value;
            overridden.push("prompts.jailbreak_prompt".to_string());
        }

        let llm = self.llm;
        let sources = llm.chat_completion_sources;
        if let Some(value) = sources.allowlist {
            capabilities.llm.chat_completion_sources.allowlist = value;
            overridden.push("llm.chat_completion_sources.allowlist".to_string());
        }

        let features = llm.chat_completion_features;
        if let Some(value) = features.web_search {
            capabilities.llm.chat_completion_features.web_search = value;
            overridden.push("llm.chat_completion_features.web_search".to_string());
        }
        if let Some(value) = features.request_images {
            capabilities.llm.chat_completion_features.request_images = value;
            overridden.push("llm.chat_completion_features.request_images".to_string());
        }

        if let Some(value) = llm.endpoint_overrides {
            capabilities.llm.endpoint_overrides = value;
            overridden.push("llm.endpoint_overrides".to_string());
        }

        let text_completions = llm.text_completions;
        if let Some(value) = text_completions.enabled {
            capabilities.llm.text_completions.enabled = value;
            overridden.push("llm.text_completions.enabled".to_string());
        }

        let network = self.network;
        if let Some(value) = network.request_proxy {
            capabilities.network.request_proxy = value;
            overridden.push("network.request_proxy".to_string());
        }

        let scripting = self.scripting;
        if let Some(value) = scripting.prompt_injections {
            capabilities.scripting.prompt_injections = value;
            overridden.push("scripting.prompt_injections".to_string());
        }
        if let Some(value) = scripting.tool_registration {
            capabilities.scripting.tool_registration = value;
            overridden.push("scripting.tool_registration".to_string());
        }

        let ai = self.ai;
        if let Some(value) = ai.image_generation {
            capabilities.ai.image_generation = value;
            overridden.push("ai.image_generation".to_string());
        }

        let sync = self.sync;
        if let Some(value) = sync.lan {
            capabilities.sync.lan = value;
            overridden.push("sync.lan".to_string());
        }

        overridden
    }
}

pub fn resolve_ios_policy_activation_report(
    scope: IosPolicyScope,
    raw_policy: Option<&Value>,
) -> Result<IosPolicyActivationReport, DomainError> {
    if scope == IosPolicyScope::Ignored {
        let capabilities = IosPolicyCapabilities::baseline(IosPolicyProfile::Full);
        return Ok(IosPolicyActivationReport {
            version: IOS_POLICY_VERSION,
            scope,
            profile: IosPolicyProfile::Full,
            capabilities,
            overridden_capabilities: Vec::new(),
        });
    }

    let Some(raw_policy) = raw_policy else {
        let capabilities = IosPolicyCapabilities::baseline(IosPolicyProfile::Full);
        return Ok(IosPolicyActivationReport {
            version: IOS_POLICY_VERSION,
            scope,
            profile: IosPolicyProfile::Full,
            capabilities,
            overridden_capabilities: Vec::new(),
        });
    };

    let config: IosPolicyConfig = serde_json::from_value(raw_policy.clone()).map_err(|error| {
        DomainError::InvalidData(format!("ios_policy: invalid schema: {}", error))
    })?;

    if config.version != IOS_POLICY_VERSION {
        return Err(DomainError::InvalidData(format!(
            "ios_policy: unsupported version {} (expected {})",
            config.version, IOS_POLICY_VERSION
        )));
    }

    let mut capabilities = IosPolicyCapabilities::baseline(config.profile);
    let overridden_capabilities = config.overrides.capabilities.apply(&mut capabilities);
    capabilities.validate()?;

    Ok(IosPolicyActivationReport {
        version: config.version,
        scope,
        profile: config.profile,
        capabilities,
        overridden_capabilities,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_full_profile_when_ignored() {
        let report = resolve_ios_policy_activation_report(
            IosPolicyScope::Ignored,
            Some(&serde_json::json!({
                "version": 1,
                "profile": "ios_external_beta",
                "overrides": {
                    "capabilities": {
                        "updates": { "manual_check": true }
                    }
                }
            })),
        )
        .unwrap();

        assert_eq!(report.scope, IosPolicyScope::Ignored);
        assert_eq!(report.profile, IosPolicyProfile::Full);
        assert!(report.capabilities.extensions.third_party_management);
    }

    #[test]
    fn rejects_unknown_keys() {
        let error = resolve_ios_policy_activation_report(
            IosPolicyScope::Ios,
            Some(&serde_json::json!({
                "version": 1,
                "profile": "full",
                "overrides": {
                    "capabilities": {
                        "llm": {
                            "chat_completion_sources": { "allowlist": ["openai"] },
                            "typo_key": true
                        }
                    }
                }
            })),
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("unknown field"),
            "unexpected error: {}",
            error
        );
    }

    #[test]
    fn applies_overrides_and_tracks_paths() {
        let report = resolve_ios_policy_activation_report(
            IosPolicyScope::Ios,
            Some(&serde_json::json!({
                "version": 1,
                "profile": "ios_external_beta",
                "overrides": {
                    "capabilities": {
                        "updates": { "manual_check": true },
                        "llm": { "endpoint_overrides": true }
                    }
                }
            })),
        )
        .unwrap();

        assert_eq!(report.profile, IosPolicyProfile::IosExternalBeta);
        assert!(report.capabilities.updates.manual_check);
        assert!(report.capabilities.llm.endpoint_overrides);
        assert!(
            report
                .overridden_capabilities
                .contains(&"updates.manual_check".to_string())
        );
        assert!(
            report
                .overridden_capabilities
                .contains(&"llm.endpoint_overrides".to_string())
        );
    }
}
