use serde::Serialize;
use ttsync_contract::dataset::DATASET_POLICY_VERSION;
use ttsync_core::dataset::{
    supported_dataset_ids, supported_profile_ids, tauri_tavern_default_selection,
};

use crate::presentation::commands::helpers::log_command;

#[derive(Debug, Clone, Serialize)]
pub struct SyncDatasetCatalogDto {
    pub policy_version: u32,
    pub supported_dataset_ids: Vec<String>,
    pub supported_profile_ids: Vec<String>,
    pub default_dataset_ids: Vec<String>,
}

#[tauri::command]
pub async fn sync_get_dataset_catalog() -> SyncDatasetCatalogDto {
    log_command("sync_get_dataset_catalog");
    SyncDatasetCatalogDto {
        policy_version: DATASET_POLICY_VERSION,
        supported_dataset_ids: supported_dataset_ids(),
        supported_profile_ids: supported_profile_ids(),
        default_dataset_ids: tauri_tavern_default_selection().dataset_ids,
    }
}
