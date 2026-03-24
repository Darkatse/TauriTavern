use std::sync::Arc;

use futures_util::TryStreamExt;
use tokio::task::JoinSet;
use tokio_util::io::StreamReader;

use ttsync_contract::manifest::ManifestEntryV2;
use ttsync_contract::peer::DeviceId;
use ttsync_contract::plan::{PlanId, SyncPlan};
use ttsync_contract::session::SessionToken;
use ttsync_contract::sync::SyncPhase;
use ttsync_contract::sync::SyncMode;

use crate::domain::errors::DomainError;
use crate::domain::models::tt_sync::{TtSyncCompletedEvent, TtSyncDirection, TtSyncProgressEvent};
use crate::infrastructure::sync_fs;
use crate::infrastructure::tt_sync::fs::scan_manifest;
use crate::infrastructure::tt_sync::runtime::TtSyncRuntime;
use crate::infrastructure::tt_sync::transfer;
use crate::infrastructure::tt_sync::v2_api::TtSyncV2Api;

pub async fn pull_from_server(
    runtime: Arc<TtSyncRuntime>,
    server_device_id: &DeviceId,
    mode: SyncMode,
) -> Result<TtSyncCompletedEvent, DomainError> {
    let mut server = runtime.get_paired_server(server_device_id).await?;
    let identity = runtime.store.load_or_create_identity().await?;

    let api = TtSyncV2Api::new(server.base_url.clone(), server.spki_sha256.clone())?;
    let session = api
        .open_session(&identity.device_id, &identity.ed25519_seed)
        .await?;

    server.permissions = session.granted_permissions;
    runtime.upsert_paired_server(server.clone()).await?;

    if !server.permissions.read {
        return Err(DomainError::AuthenticationError(
            "TT-Sync server does not grant read permission".to_string(),
        ));
    }
    if mode == SyncMode::Mirror && !server.permissions.mirror_delete {
        return Err(DomainError::AuthenticationError(
            "TT-Sync server does not grant mirror_delete permission".to_string(),
        ));
    }

    runtime.emit_progress(TtSyncProgressEvent {
        direction: TtSyncDirection::Pull,
        phase: SyncPhase::Scanning,
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        current_path: None,
    })?;

    let target_manifest = scan_manifest(runtime.sync_root.clone()).await?;

    runtime.emit_progress(TtSyncProgressEvent {
        direction: TtSyncDirection::Pull,
        phase: SyncPhase::Diffing,
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        current_path: None,
    })?;

    let plan = api
        .pull_plan(&session.session_token, mode, target_manifest)
        .await?;
    let plan_files_total = plan.files_total;
    let plan_bytes_total = plan.bytes_total;

    let files_deleted = apply_pull_plan(&runtime, api, &session.session_token, plan, mode).await?;

    let mut updated = server;
    updated.last_sync_ms = Some(transfer::now_ms());
    runtime.upsert_paired_server(updated).await?;

    Ok(TtSyncCompletedEvent {
        direction: TtSyncDirection::Pull,
        files_total: plan_files_total,
        bytes_total: plan_bytes_total,
        files_deleted,
    })
}

async fn apply_pull_plan(
    runtime: &TtSyncRuntime,
    api: TtSyncV2Api,
    session_token: &SessionToken,
    plan: SyncPlan,
    mode: SyncMode,
) -> Result<usize, DomainError> {
    let mut files_done = 0usize;
    let mut bytes_done = 0u64;
    let files_total = plan.files_total;
    let bytes_total = plan.bytes_total;

    runtime.emit_progress(TtSyncProgressEvent {
        direction: TtSyncDirection::Pull,
        phase: SyncPhase::Downloading,
        files_done,
        files_total,
        bytes_done,
        bytes_total,
        current_path: None,
    })?;

    let download_concurrency = transfer::default_transfer_concurrency();
    let mut join_set = JoinSet::new();
    let mut download_iter = plan.transfer.into_iter();
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
            plan.plan_id.clone(),
            entry,
        );
        in_flight += 1;
    }

    while in_flight > 0 {
        let joined = join_set
            .join_next()
            .await
            .ok_or_else(|| DomainError::InternalError("Download join set ended early".to_string()))?
            .map_err(|error| DomainError::InternalError(error.to_string()))??;

        in_flight -= 1;
        files_done += 1;
        bytes_done += joined.size_bytes;

        if transfer::should_emit_progress(files_done, files_total) {
            runtime.emit_progress(TtSyncProgressEvent {
                direction: TtSyncDirection::Pull,
                phase: SyncPhase::Downloading,
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
                plan.plan_id.clone(),
                entry,
            );
            in_flight += 1;
        }
    }

    if mode != SyncMode::Mirror || plan.delete.is_empty() {
        return Ok(0);
    }

    let delete_total = plan.delete.len();
    runtime.emit_progress(TtSyncProgressEvent {
        direction: TtSyncDirection::Pull,
        phase: SyncPhase::Deleting,
        files_done: 0,
        files_total: delete_total,
        bytes_done: 0,
        bytes_total: 0,
        current_path: None,
    })?;

    let mut files_deleted = 0usize;
    for sync_path in plan.delete {
        let full_path = transfer::resolve_to_local(&runtime.sync_root, &sync_path);
        tokio::fs::remove_file(&full_path)
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        files_deleted += 1;
        if transfer::should_emit_progress(files_deleted, delete_total) {
            runtime.emit_progress(TtSyncProgressEvent {
                direction: TtSyncDirection::Pull,
                phase: SyncPhase::Deleting,
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
    api: TtSyncV2Api,
    sync_root: std::path::PathBuf,
    session_token: SessionToken,
    plan_id: PlanId,
    entry: ManifestEntryV2,
) {
    join_set.spawn(async move {
        download_one(&api, &sync_root, &session_token, &plan_id, entry).await
    });
}

async fn download_one(
    api: &TtSyncV2Api,
    sync_root: &std::path::Path,
    session_token: &SessionToken,
    plan_id: &PlanId,
    entry: ManifestEntryV2,
) -> Result<DownloadResult, DomainError> {
    let full_path = transfer::resolve_to_local(sync_root, &entry.path);

    let response = api.download_file(session_token, plan_id, &entry.path).await?;
    let stream = response
        .bytes_stream()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error));
    let mut reader = StreamReader::new(stream);

    sync_fs::write_file_atomic(&full_path, &mut reader, entry.modified_ms).await?;

    Ok(DownloadResult {
        path: entry.path.to_string(),
        size_bytes: entry.size_bytes,
    })
}
