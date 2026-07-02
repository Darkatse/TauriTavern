#[cfg(test)]
use ttsync_contract::sync::SyncMode;
use ttsync_core::dataset::ResolvedDatasetPolicy;
#[cfg(test)]
use ttsync_core::dataset::tauri_tavern_default_selection;

use tt_contracts::sync::SyncOperationOptions;
use tt_contracts::sync_automation::{
    SYNC_AUTOMATION_MAX_INTERVAL_MINUTES, SYNC_AUTOMATION_MIN_INTERVAL_MINUTES, ScheduledSyncRule,
    SyncAutomationConfig,
};
use tt_domain::errors::DomainError;

#[cfg(test)]
pub fn default_sync_operation_options() -> SyncOperationOptions {
    SyncOperationOptions {
        selection: tauri_tavern_default_selection(),
        require_bundle_zstd: false,
    }
}

pub fn validate_sync_operation_options(
    options: SyncOperationOptions,
) -> Result<SyncOperationOptions, DomainError> {
    validate_dataset_selection(&options.selection)?;
    Ok(options)
}

#[cfg(test)]
pub fn default_scheduled_sync_rule() -> ScheduledSyncRule {
    ScheduledSyncRule {
        enabled: false,
        interval_minutes: 30,
        target: None,
        sync_mode: SyncMode::Incremental,
        selection: tauri_tavern_default_selection(),
        require_bundle_zstd: true,
    }
}

pub fn validate_scheduled_sync_rule(rule: &ScheduledSyncRule) -> Result<(), DomainError> {
    if rule.interval_minutes < SYNC_AUTOMATION_MIN_INTERVAL_MINUTES
        || rule.interval_minutes > SYNC_AUTOMATION_MAX_INTERVAL_MINUTES
    {
        return Err(DomainError::InvalidData(format!(
            "Auto sync interval must be between {} and {} minutes",
            SYNC_AUTOMATION_MIN_INTERVAL_MINUTES, SYNC_AUTOMATION_MAX_INTERVAL_MINUTES
        )));
    }

    if rule.enabled && rule.target.is_none() {
        return Err(DomainError::InvalidData(
            "Auto sync target is required when auto sync is enabled".to_string(),
        ));
    }

    validate_dataset_selection(&rule.selection)
}

pub fn validate_sync_automation_config(config: &SyncAutomationConfig) -> Result<(), DomainError> {
    validate_scheduled_sync_rule(&config.clone().into_rule())
}

fn validate_dataset_selection(
    selection: &ttsync_contract::dataset::DatasetSelection,
) -> Result<(), DomainError> {
    ResolvedDatasetPolicy::from_selection(selection)
        .map_err(|error| DomainError::InvalidData(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use ttsync_contract::dataset::{DATASET_POLICY_VERSION, DatasetSelection};

    use super::*;
    use tt_contracts::sync_automation::SyncAutomationTarget;

    #[test]
    fn default_sync_options_validate() {
        let options = validate_sync_operation_options(default_sync_operation_options())
            .expect("default sync options should be valid");

        assert!(!options.require_bundle_zstd);
    }

    #[test]
    fn invalid_sync_selection_is_rejected() {
        let options = SyncOperationOptions {
            selection: DatasetSelection::new(DATASET_POLICY_VERSION, vec!["missing".to_string()]),
            require_bundle_zstd: false,
        };

        assert!(matches!(
            validate_sync_operation_options(options),
            Err(DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn scheduled_rule_rejects_too_frequent_interval() {
        let rule = ScheduledSyncRule {
            interval_minutes: SYNC_AUTOMATION_MIN_INTERVAL_MINUTES - 1,
            ..default_scheduled_sync_rule()
        };

        assert!(matches!(
            validate_scheduled_sync_rule(&rule),
            Err(DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn scheduled_rule_rejects_enabled_rule_without_target() {
        let rule = ScheduledSyncRule {
            enabled: true,
            ..default_scheduled_sync_rule()
        };

        assert!(matches!(
            validate_scheduled_sync_rule(&rule),
            Err(DomainError::InvalidData(_))
        ));
    }

    #[test]
    fn scheduled_rule_accepts_default_selection_with_target() {
        let rule = ScheduledSyncRule {
            enabled: true,
            target: Some(SyncAutomationTarget::Lan {
                device_id: "11111111-1111-4111-8111-111111111111".to_string(),
            }),
            ..default_scheduled_sync_rule()
        };

        validate_scheduled_sync_rule(&rule).expect("valid scheduled rule");
    }
}
