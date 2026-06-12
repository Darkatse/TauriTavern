use std::sync::Arc;

use async_compression::tokio::bufread::ZstdDecoder;
use futures_util::TryStreamExt;
use tokio::io::BufReader;
use tokio::task::JoinSet;
use tokio_util::io::StreamReader;
use ttsync_contract::manifest::ManifestEntryV2;
use ttsync_contract::peer::DeviceId;
use ttsync_contract::plan::{PlanId, SyncPlan};
use ttsync_contract::session::SessionToken;
use ttsync_contract::sync::SyncMode;
use ttsync_core::dataset::{ResolvedDatasetPolicy, tauri_tavern_default_selection};

use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{
    LanSyncSyncCompletedEvent, LanSyncSyncMode, LanSyncSyncPhase, LanSyncSyncProgressEvent,
};
use crate::infrastructure::lan_sync::runtime::LanSyncRuntime;
use crate::infrastructure::lan_sync::v2::client::LanSyncV2Api;
use crate::infrastructure::lan_sync::v2::store::LanSyncV2Store;
use crate::infrastructure::sync_bundle::{
    BUNDLE_ZSTD_DECODE_BUFFER_SIZE, FEATURE_BUNDLE_V1, FEATURE_ZSTD_V1, write_bundle_to_local_files,
};
use crate::infrastructure::sync_fs;
use crate::infrastructure::sync_transfer;
use crate::infrastructure::tt_sync::fs::{scan_manifest_with_policy, validate_plan_scope};
use crate::infrastructure::tt_sync::v2_api::ensure_dataset_scope_v1;

pub async fn pull_from_device(
    runtime: Arc<LanSyncRuntime>,
    store: LanSyncV2Store,
    device_id: &DeviceId,
) -> Result<LanSyncSyncCompletedEvent, DomainError> {
    let mut peer = store.get_paired_device(device_id).await?;
    let identity = store.load_or_create_identity().await?;
    let mode = effective_sync_mode(&runtime).await?;

    let api = LanSyncV2Api::new(peer.base_url.clone(), peer.spki_sha256.clone())?;
    let status = api.status().await?;
    ensure_dataset_scope_v1(&status, "LAN Sync v2 peer")?;
    let prefer_bundle = status
        .features
        .iter()
        .any(|feature| feature == FEATURE_BUNDLE_V1);
    let accept_zstd = prefer_bundle
        && status
            .features
            .iter()
            .any(|feature| feature == FEATURE_ZSTD_V1);

    let session = api
        .open_session(&identity.device_id, &identity.ed25519_seed)
        .await?;
    peer.grant.permissions = session.granted_permissions;
    store.upsert_paired_device(peer.clone()).await?;

    if !peer.grant.permissions.read {
        return Err(DomainError::AuthenticationError(
            "LAN Sync v2 peer does not grant read permission".to_string(),
        ));
    }
    if mode == SyncMode::Mirror && !peer.grant.permissions.mirror_delete {
        return Err(DomainError::AuthenticationError(
            "LAN Sync v2 peer does not grant mirror_delete permission".to_string(),
        ));
    }

    runtime.emit_sync_progress(LanSyncSyncProgressEvent {
        phase: LanSyncSyncPhase::Scanning,
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        current_path: None,
    })?;

    let selection = tauri_tavern_default_selection();
    let policy = ResolvedDatasetPolicy::from_selection(&selection)
        .map_err(|error| DomainError::InvalidData(error.to_string()))?;
    let target_manifest =
        scan_manifest_with_policy(runtime.sync_root.clone(), policy.clone()).await?;

    runtime.emit_sync_progress(LanSyncSyncProgressEvent {
        phase: LanSyncSyncPhase::Diffing,
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        current_path: None,
    })?;

    let plan = api
        .pull_plan(&session.session_token, mode, selection, target_manifest)
        .await?;
    validate_plan_scope(&plan, &policy)?;
    let files_total = plan.files_total;
    let bytes_total = plan.bytes_total;

    let files_deleted = apply_pull_plan(
        &runtime,
        api,
        &session.session_token,
        plan,
        mode,
        prefer_bundle,
        accept_zstd,
    )
    .await?;

    let mut updated_peer = peer;
    updated_peer.grant.last_sync_ms = Some(sync_transfer::now_ms());
    store.upsert_paired_device(updated_peer).await?;

    Ok(LanSyncSyncCompletedEvent {
        files_total,
        bytes_total,
        files_deleted,
    })
}

async fn effective_sync_mode(runtime: &LanSyncRuntime) -> Result<SyncMode, DomainError> {
    let config = runtime.store.load_or_create_config().await?;
    let mode = runtime
        .get_sync_mode_override()
        .await
        .unwrap_or(config.sync_mode);
    Ok(lan_sync_mode_to_v2(mode))
}

pub fn lan_sync_mode_to_v2(mode: LanSyncSyncMode) -> SyncMode {
    match mode {
        LanSyncSyncMode::Incremental => SyncMode::Incremental,
        LanSyncSyncMode::Mirror => SyncMode::Mirror,
    }
}

async fn apply_pull_plan(
    runtime: &LanSyncRuntime,
    api: LanSyncV2Api,
    session_token: &SessionToken,
    plan: SyncPlan,
    mode: SyncMode,
    prefer_bundle: bool,
    accept_zstd: bool,
) -> Result<usize, DomainError> {
    let plan_id = plan.plan_id;
    let transfer_entries = plan.transfer;
    let delete = plan.delete;
    let mut files_done = 0usize;
    let mut bytes_done = 0u64;
    let files_total = transfer_entries.len();
    let bytes_total = transfer_entries.iter().map(|entry| entry.size_bytes).sum();

    runtime.emit_sync_progress(LanSyncSyncProgressEvent {
        phase: LanSyncSyncPhase::Downloading,
        files_done,
        files_total,
        bytes_done,
        bytes_total,
        current_path: None,
    })?;

    if prefer_bundle && !transfer_entries.is_empty() {
        let response = api
            .download_bundle(session_token, &plan_id, accept_zstd)
            .await?;
        let content_encoding = response
            .headers()
            .get(reqwest::header::CONTENT_ENCODING)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        let is_zstd = content_encoding.eq_ignore_ascii_case("zstd");

        let stream = response.bytes_stream().map_err(std::io::Error::other);
        let reader = StreamReader::new(stream);
        let mut reader: Box<dyn tokio::io::AsyncRead + Send + Unpin> = if is_zstd {
            Box::new(ZstdDecoder::new(BufReader::with_capacity(
                BUNDLE_ZSTD_DECODE_BUFFER_SIZE,
                reader,
            )))
        } else {
            Box::new(reader)
        };

        write_bundle_to_local_files(
            &runtime.sync_root,
            transfer_entries,
            &mut reader,
            |progress| {
                files_done += 1;
                bytes_done += progress.size_bytes;

                if sync_transfer::should_emit_progress(files_done, files_total) {
                    runtime.emit_sync_progress(LanSyncSyncProgressEvent {
                        phase: LanSyncSyncPhase::Downloading,
                        files_done,
                        files_total,
                        bytes_done,
                        bytes_total,
                        current_path: Some(progress.path),
                    })?;
                }

                Ok(())
            },
        )
        .await?;
    } else {
        let download_concurrency = sync_transfer::default_transfer_concurrency();
        let mut join_set = JoinSet::new();
        let mut download_iter = transfer_entries.into_iter();
        let mut in_flight = 0usize;

        while in_flight < download_concurrency {
            let Some(entry) = download_iter.next() else {
                break;
            };

            spawn_download_task(
                &mut join_set,
                api.clone(),
                runtime.sync_root.clone(),
                session_token.clone(),
                plan_id.clone(),
                entry,
            );
            in_flight += 1;
        }

        while in_flight > 0 {
            let joined = join_set
                .join_next()
                .await
                .ok_or_else(|| {
                    DomainError::InternalError("Download join set ended early".to_string())
                })?
                .map_err(|error| DomainError::InternalError(error.to_string()))??;

            in_flight -= 1;
            files_done += 1;
            bytes_done += joined.size_bytes;

            if sync_transfer::should_emit_progress(files_done, files_total) {
                runtime.emit_sync_progress(LanSyncSyncProgressEvent {
                    phase: LanSyncSyncPhase::Downloading,
                    files_done,
                    files_total,
                    bytes_done,
                    bytes_total,
                    current_path: Some(joined.path),
                })?;
            }

            if let Some(entry) = download_iter.next() {
                spawn_download_task(
                    &mut join_set,
                    api.clone(),
                    runtime.sync_root.clone(),
                    session_token.clone(),
                    plan_id.clone(),
                    entry,
                );
                in_flight += 1;
            }
        }
    }

    if mode != SyncMode::Mirror || delete.is_empty() {
        return Ok(0);
    }

    let delete_total = delete.len();
    runtime.emit_sync_progress(LanSyncSyncProgressEvent {
        phase: LanSyncSyncPhase::Deleting,
        files_done: 0,
        files_total: delete_total,
        bytes_done: 0,
        bytes_total: 0,
        current_path: None,
    })?;

    let mut files_deleted = 0usize;
    for sync_path in delete {
        let full_path = sync_transfer::resolve_to_local(&runtime.sync_root, &sync_path);
        tokio::fs::remove_file(&full_path)
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        files_deleted += 1;
        if sync_transfer::should_emit_progress(files_deleted, delete_total) {
            runtime.emit_sync_progress(LanSyncSyncProgressEvent {
                phase: LanSyncSyncPhase::Deleting,
                files_done: files_deleted,
                files_total: delete_total,
                bytes_done: 0,
                bytes_total: 0,
                current_path: Some(sync_path.to_string()),
            })?;
        }
    }

    Ok(files_deleted)
}

struct DownloadResult {
    path: String,
    size_bytes: u64,
}

fn spawn_download_task(
    join_set: &mut JoinSet<Result<DownloadResult, DomainError>>,
    api: LanSyncV2Api,
    sync_root: std::path::PathBuf,
    session_token: SessionToken,
    plan_id: PlanId,
    entry: ManifestEntryV2,
) {
    join_set.spawn(
        async move { download_one(&api, &sync_root, &session_token, &plan_id, entry).await },
    );
}

async fn download_one(
    api: &LanSyncV2Api,
    sync_root: &std::path::Path,
    session_token: &SessionToken,
    plan_id: &PlanId,
    entry: ManifestEntryV2,
) -> Result<DownloadResult, DomainError> {
    let full_path = sync_transfer::resolve_to_local(sync_root, &entry.path);
    let response = api
        .download_file(session_token, plan_id, &entry.path)
        .await?;
    let stream = response.bytes_stream().map_err(std::io::Error::other);
    let mut reader = StreamReader::new(stream);

    sync_fs::write_file_atomic(&full_path, &mut reader, entry.modified_ms).await?;

    Ok(DownloadResult {
        path: entry.path.to_string(),
        size_bytes: entry.size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::lan_sync_mode_to_v2;

    use crate::domain::models::lan_sync::LanSyncSyncMode;

    #[test]
    fn lan_sync_mode_maps_to_v2_sync_mode() {
        assert_eq!(
            lan_sync_mode_to_v2(LanSyncSyncMode::Incremental),
            ttsync_contract::sync::SyncMode::Incremental
        );
        assert_eq!(
            lan_sync_mode_to_v2(LanSyncSyncMode::Mirror),
            ttsync_contract::sync::SyncMode::Mirror
        );
    }
}
