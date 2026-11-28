use std::fmt;

use serde_json::value::{RawValue, to_raw_value};

use crate::json_document::JsonDocument;

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
const PROMPT_MANAGER_SETTINGS: &str = "oai_settings";
const PROMPTS: &str = "prompts";
const PROMPT_ORDER: &str = "prompt_order";
const ORDER: &str = "order";

/// Entries are compared and kept as text, so a prompt is never decoded;
/// a member that is not a list stays as it is.
pub fn repair_prompt_manager_settings(settings: &mut JsonDocument) -> UserSettingsRepairReport {
    let mut report = UserSettingsRepairReport::default();
    repair_prompt_lists(settings, &mut report);
    if let Ok(Some(prompt_manager)) = settings.object_mut(&[PROMPT_MANAGER_SETTINGS]) {
        repair_prompt_lists(prompt_manager, &mut report);
    }
    report
}

fn repair_prompt_lists(settings: &mut JsonDocument, report: &mut UserSettingsRepairReport) {
    if let Some(mut prompts) = list(settings, PROMPTS) {
        let removed = remove_nulls(&mut prompts);
        if removed > 0 {
            report.null_prompts += removed;
            set_list(settings, PROMPTS, &prompts);
        }
    }

    let Some(mut prompt_order) = list(settings, PROMPT_ORDER) else {
        return;
    };
    let removed_lists = remove_nulls(&mut prompt_order);
    let removed_references: usize = prompt_order.iter_mut().map(repair_order).sum();
    if removed_lists + removed_references > 0 {
        report.null_prompt_order_lists += removed_lists;
        report.null_prompt_order_references += removed_references;
        set_list(settings, PROMPT_ORDER, &prompt_order);
    }
}

/// Returns how many null references left the entry's `order` list.
fn repair_order(entry: &mut Box<RawValue>) -> usize {
    let Ok(mut fields) = JsonDocument::parse(entry.get().as_bytes()) else {
        return 0;
    };
    let Some(mut order) = list(&fields, ORDER) else {
        return 0;
    };
    let removed = remove_nulls(&mut order);
    if removed > 0 {
        set_list(&mut fields, ORDER, &order);
        *entry = to_raw_value(&fields).expect("a document serializes");
    }
    removed
}

fn list(settings: &JsonDocument, field: &str) -> Option<Vec<Box<RawValue>>> {
    settings.get(&[field]).ok().flatten()
}

fn remove_nulls(list: &mut Vec<Box<RawValue>>) -> usize {
    let before = list.len();
    list.retain(|entry| entry.get() != "null");
    before - list.len()
}

fn set_list(settings: &mut JsonDocument, field: &str, list: &[Box<RawValue>]) {
    settings.set(field, to_raw_value(list).expect("raw JSON serializes"));
}
