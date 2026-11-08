use std::fmt;

use serde_json::{Map, Value};

use crate::models::settings::UserSettings;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UserSettingsRepairReport {
    null_prompts: usize,
    null_prompt_order_lists: usize,
    null_prompt_order_references: usize,
}

impl UserSettingsRepairReport {
    pub fn changed(&self) -> bool {
        self.null_prompts > 0
            || self.null_prompt_order_lists > 0
            || self.null_prompt_order_references > 0
    }
}

impl fmt::Display for UserSettingsRepairReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "removed {} null prompt entries, {} null prompt_order entries, {} null prompt_order.order entries",
            self.null_prompts, self.null_prompt_order_lists, self.null_prompt_order_references
        )
    }
}

/// The settings objects holding PromptManager lists: the settings root and this member of it.
pub const PROMPT_MANAGER_SETTINGS: &str = "oai_settings";
/// The only fields of a PromptManager settings object that the repair reads.
pub const PROMPT_MANAGER_LISTS: [&str; 2] = [PROMPTS, PROMPT_ORDER];
const PROMPTS: &str = "prompts";
const PROMPT_ORDER: &str = "prompt_order";

pub fn repair_sillytavern_prompt_manager_settings(
    settings: &mut UserSettings,
) -> UserSettingsRepairReport {
    let mut report = UserSettingsRepairReport::default();
    let Some(root) = settings.data.as_object_mut() else {
        return report;
    };

    repair_prompt_manager_object(root, &mut report);

    if let Some(oai_settings) = root
        .get_mut(PROMPT_MANAGER_SETTINGS)
        .and_then(Value::as_object_mut)
    {
        repair_prompt_manager_object(oai_settings, &mut report);
    }

    report
}

/// Repairs one PromptManager settings object; only its [`PROMPT_MANAGER_LISTS`] are read.
pub fn repair_prompt_manager_object(
    settings: &mut Map<String, Value>,
    report: &mut UserSettingsRepairReport,
) {
    if let Some(prompts) = settings.get_mut(PROMPTS).and_then(Value::as_array_mut) {
        let before = prompts.len();
        prompts.retain(|prompt| !prompt.is_null());
        report.null_prompts += before - prompts.len();
    }

    let Some(prompt_order) = settings.get_mut(PROMPT_ORDER).and_then(Value::as_array_mut) else {
        return;
    };

    let before = prompt_order.len();
    prompt_order.retain(|order| !order.is_null());
    report.null_prompt_order_lists += before - prompt_order.len();

    for order in prompt_order {
        let Some(order_entries) = order.get_mut("order").and_then(Value::as_array_mut) else {
            continue;
        };

        let before = order_entries.len();
        order_entries.retain(|entry| !entry.is_null());
        report.null_prompt_order_references += before - order_entries.len();
    }
}
