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
use ttsync_core::dataset::ResolvedDatasetPolicy;

use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{LanSyncSyncPhase, LanSyncSyncProgressEvent};
use crate::domain::models::sync::{
    LocalAppliedChangeSummary, SyncExecutionFailure, SyncExecutionReport, SyncJobSummary,
    SyncOperationOptions,
};
use crate::infrastructure::lan_sync::runtime::LanSyncRuntime;
use crate::infrastructure::sync::bundle_transport_for_status;
use crate::infrastructure::sync::http_client::ensure_dataset_scope_v1;
use crate::infrastructure::sync::lan::client::LanSyncClient;
use crate::infrastructure::sync::lan::store::LanPeerStore;
use crate::infrastructure::sync_bundle::{
    BUNDLE_ZSTD_DECODE_BUFFER_SIZE, write_bundle_to_local_files,
};
use crate::infrastructure::sync_fs;
use crate::infrastructure::sync_transfer;
use crate::infrastructure::tt_sync::fs::{scan_manifest_with_policy, validate_plan_scope};

pub async fn pull_from_device(
    runtime: Arc<LanSyncRuntime>,
    store: LanPeerStore,
    device_id: &DeviceId,
    mode: SyncMode,
    options: SyncOperationOptions,
) -> Result<SyncExecutionReport, SyncExecutionFailure> {
    let mut peer = store.get_paired_device(device_id).await?;
    let identity = store.load_or_create_identity().await?;

    let api = LanSyncClient::new(peer.base_url.clone(), peer.spki_sha256.clone())?;
    let status = api.status().await?;
    ensure_dataset_scope_v1(&status, "LAN Sync peer")?;
    let transport =
        bundle_transport_for_status(&status, "LAN Sync peer", options.require_bundle_zstd)?;

    let session = api
        .open_session(&identity.device_id, &identity.ed25519_seed)
        .await?;
    peer.grant.permissions = session.granted_permissions;
    store.upsert_paired_device(peer.clone()).await?;

    if !peer.grant.permissions.read {
        return Err(DomainError::AuthenticationError(
            "LAN Sync peer does not grant read permission".to_string(),
        )
        .into());
    }
    if mode == SyncMode::Mirror && !peer.grant.permissions.mirror_delete {
        return Err(DomainError::AuthenticationError(
            "LAN Sync peer does not grant mirror_delete permission".to_string(),
        )
        .into());
    }

    runtime.emit_sync_progress(LanSyncSyncProgressEvent {
        phase: LanSyncSyncPhase::Scanning,
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        current_path: None,
    });

    let selection = options.selection;
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
    });

    let plan = api
        .pull_plan(&session.session_token, mode, selection, target_manifest)
        .await?;
    validate_plan_scope(&plan, &policy)?;
    let files_total = plan.files_total;
    let bytes_total = plan.bytes_total;

    let local_applied = apply_pull_plan(
        &runtime,
        api,
        &session.session_token,
        plan,
        mode,
        transport.prefer_bundle,
        transport.use_zstd,
    )
    .await?;

    let mut updated_peer = peer;
    updated_peer.grant.last_sync_ms = Some(sync_transfer::now_ms());
    store
        .upsert_paired_device(updated_peer)
        .await
        .map_err(|error| SyncExecutionFailure::new(error, local_applied))?;

    Ok(SyncExecutionReport::completed(
        SyncJobSummary::new(files_total, bytes_total, local_applied.files_deleted),
        local_applied,
    ))
}

async fn apply_pull_plan(
    runtime: &LanSyncRuntime,
    api: LanSyncClient,
    session_token: &SessionToken,
    plan: SyncPlan,
    mode: SyncMode,
    prefer_bundle: bool,
    accept_zstd: bool,
) -> Result<LocalAppliedChangeSummary, SyncExecutionFailure> {
    let plan_id = plan.plan_id;
    let transfer_entries = plan.transfer;
    let delete = plan.delete;
    let tracker = Arc::new(sync_transfer::LocalChangeTracker::default());
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
    });

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

        if let Err(error) = write_bundle_to_local_files(
            &runtime.sync_root,
            transfer_entries,
            &mut reader,
            |progress| {
                files_done += 1;
                bytes_done += progress.size_bytes;
                tracker.record_write(progress.size_bytes);

                if sync_transfer::should_emit_progress(files_done, files_total) {
                    runtime.emit_sync_progress(LanSyncSyncProgressEvent {
                        phase: LanSyncSyncPhase::Downloading,
                        files_done,
                        files_total,
                        bytes_done,
                        bytes_total,
                        current_path: Some(progress.path),
                    });
                }

                Ok(())
            },
        )
        .await
        {
            if error.target_changed() {
                tracker.record_delete();
            }
            return Err(SyncExecutionFailure::new(
                error.into_error(),
                tracker.summary(),
            ));
        }
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
                tracker.clone(),
            );
            in_flight += 1;
        }

        let mut first_error = None;
        while in_flight > 0 {
            let joined = match join_set.join_next().await {
                Some(Ok(Ok(joined))) => Some(joined),
                Some(Ok(Err(error))) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    None
                }
                Some(Err(error)) => {
                    if first_error.is_none() {
                        first_error = Some(DomainError::InternalError(error.to_string()));
                    }
                    None
                }
                None => {
                    if first_error.is_none() {
                        first_error = Some(DomainError::InternalError(
                            "Download join set ended early".to_string(),
                        ));
                    }
                    None
                }
            };

            in_flight -= 1;
            if let Some(joined) = joined {
                if first_error.is_none() {
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
                        });
                    }
                }
            }

            if first_error.is_none()
                && let Some(entry) = download_iter.next()
            {
                spawn_download_task(
                    &mut join_set,
                    api.clone(),
                    runtime.sync_root.clone(),
                    session_token.clone(),
                    plan_id.clone(),
                    entry,
                    tracker.clone(),
                );
                in_flight += 1;
            }
        }

        if let Some(error) = first_error {
            return Err(SyncExecutionFailure::new(error, tracker.summary()));
        }
    }

    if mode != SyncMode::Mirror || delete.is_empty() {
        return Ok(tracker.summary());
    }

    let delete_total = delete.len();
    runtime.emit_sync_progress(LanSyncSyncProgressEvent {
        phase: LanSyncSyncPhase::Deleting,
        files_done: 0,
        files_total: delete_total,
        bytes_done: 0,
        bytes_total: 0,
        current_path: None,
    });

    let mut files_deleted = 0usize;
    for sync_path in delete {
        let full_path = sync_transfer::resolve_to_local(&runtime.sync_root, &sync_path);
        if let Err(error) = tokio::fs::remove_file(&full_path).await {
            return Err(SyncExecutionFailure::new(
                DomainError::InternalError(error.to_string()),
                tracker.summary(),
            ));
        }

        files_deleted += 1;
        tracker.record_delete();
        if sync_transfer::should_emit_progress(files_deleted, delete_total) {
            runtime.emit_sync_progress(LanSyncSyncProgressEvent {
                phase: LanSyncSyncPhase::Deleting,
                files_done: files_deleted,
                files_total: delete_total,
                bytes_done: 0,
                bytes_total: 0,
                current_path: Some(sync_path.to_string()),
            });
        }
    }

    Ok(tracker.summary())
}

struct DownloadResult {
    path: String,
    size_bytes: u64,
}

fn spawn_download_task(
    join_set: &mut JoinSet<Result<DownloadResult, DomainError>>,
    api: LanSyncClient,
    sync_root: std::path::PathBuf,
    session_token: SessionToken,
    plan_id: PlanId,
    entry: ManifestEntryV2,
    tracker: Arc<sync_transfer::LocalChangeTracker>,
) {
    join_set.spawn(async move {
        download_one(&api, &sync_root, &session_token, &plan_id, entry, tracker).await
    });
}

async fn download_one(
    api: &LanSyncClient,
    sync_root: &std::path::Path,
    session_token: &SessionToken,
    plan_id: &PlanId,
    entry: ManifestEntryV2,
    tracker: Arc<sync_transfer::LocalChangeTracker>,
) -> Result<DownloadResult, DomainError> {
    let full_path = sync_transfer::resolve_to_local(sync_root, &entry.path);
    let response = api
        .download_file(session_token, plan_id, &entry.path)
        .await?;
    let stream = response.bytes_stream().map_err(std::io::Error::other);
    let mut reader = StreamReader::new(stream);

    let size_bytes = entry.size_bytes;
    if let Err(error) = sync_fs::write_file_atomic(&full_path, &mut reader, entry.modified_ms).await
    {
        if error.target_changed() {
            tracker.record_delete();
        }
        return Err(error.into_error());
    }
    tracker.record_write(size_bytes);

    Ok(DownloadResult {
        path: entry.path.to_string(),
        size_bytes,
    })
}
