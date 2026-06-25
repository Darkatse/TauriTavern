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
use ttsync_contract::sync::SyncPhase;
use ttsync_core::dataset::ResolvedDatasetPolicy;

use crate::domain::errors::DomainError;
use crate::domain::models::sync::{
    LocalAppliedChangeSummary, SyncExecutionFailure, SyncExecutionReport, SyncJobSummary,
    SyncOperationOptions, SyncOrigin,
};
use crate::domain::models::tt_sync::{TtSyncDirection, TtSyncProgressEvent};
use crate::infrastructure::sync::bundle_transport_for_status;
use crate::infrastructure::sync::http_client::{SyncHttpClient, ensure_dataset_scope_v1};
use crate::infrastructure::sync_bundle::{
    BUNDLE_ZSTD_DECODE_BUFFER_SIZE, write_bundle_to_local_files,
};
use crate::infrastructure::sync_fs;
use crate::infrastructure::tt_sync::fs::{scan_manifest_with_policy, validate_plan_scope};
use crate::infrastructure::tt_sync::runtime::TtSyncRuntime;
use crate::infrastructure::tt_sync::transfer;

pub async fn pull_from_server(
    runtime: Arc<TtSyncRuntime>,
    server_device_id: &DeviceId,
    mode: SyncMode,
    options: SyncOperationOptions,
    origin: SyncOrigin,
) -> Result<SyncExecutionReport, SyncExecutionFailure> {
    let mut server = runtime.get_paired_server(server_device_id).await?;
    let identity = runtime.store.load_or_create_identity().await?;

    let api = SyncHttpClient::new(server.base_url.clone(), server.spki_sha256.clone())?;
    let status = api.status().await?;
    ensure_dataset_scope_v1(&status, "TT-Sync server")?;
    let transport =
        bundle_transport_for_status(&status, "TT-Sync server", options.require_bundle_zstd)?;

    let session = api
        .open_session(&identity.device_id, &identity.ed25519_seed)
        .await?;

    server.permissions = session.granted_permissions;
    runtime.upsert_paired_server(server.clone()).await?;

    if !server.permissions.read {
        return Err(DomainError::AuthenticationError(
            "TT-Sync server does not grant read permission".to_string(),
        )
        .into());
    }
    if mode == SyncMode::Mirror && !server.permissions.mirror_delete {
        return Err(DomainError::AuthenticationError(
            "TT-Sync server does not grant mirror_delete permission".to_string(),
        )
        .into());
    }

    runtime.emit_progress(
        TtSyncProgressEvent {
            direction: TtSyncDirection::Pull,
            phase: SyncPhase::Scanning,
            files_done: 0,
            files_total: 0,
            bytes_done: 0,
            bytes_total: 0,
            current_path: None,
        },
        &origin,
    );

    let selection = options.selection;
    let policy = ResolvedDatasetPolicy::from_selection(&selection)
        .map_err(|error| DomainError::InvalidData(error.to_string()))?;
    let target_manifest =
        scan_manifest_with_policy(runtime.sync_root.clone(), policy.clone()).await?;

    runtime.emit_progress(
        TtSyncProgressEvent {
            direction: TtSyncDirection::Pull,
            phase: SyncPhase::Diffing,
            files_done: 0,
            files_total: 0,
            bytes_done: 0,
            bytes_total: 0,
            current_path: None,
        },
        &origin,
    );

    let plan = api
        .pull_plan(&session.session_token, mode, selection, target_manifest)
        .await?;
    validate_plan_scope(&plan, &policy)?;
    let plan_files_total = plan.files_total;
    let plan_bytes_total = plan.bytes_total;

    let local_applied = apply_pull_plan(
        &runtime,
        api,
        &session.session_token,
        plan,
        mode,
        transport.prefer_bundle,
        transport.use_zstd,
        &origin,
    )
    .await?;

    let mut updated = server;
    updated.last_sync_ms = Some(transfer::now_ms());
    runtime
        .upsert_paired_server(updated)
        .await
        .map_err(|error| SyncExecutionFailure::new(error, local_applied))?;

    Ok(SyncExecutionReport::completed(
        SyncJobSummary::new(
            plan_files_total,
            plan_bytes_total,
            local_applied.files_deleted,
        ),
        local_applied,
    ))
}

async fn apply_pull_plan(
    runtime: &TtSyncRuntime,
    api: SyncHttpClient,
    session_token: &SessionToken,
    plan: SyncPlan,
    mode: SyncMode,
    prefer_bundle: bool,
    accept_zstd: bool,
    origin: &SyncOrigin,
) -> Result<LocalAppliedChangeSummary, SyncExecutionFailure> {
    let plan_id = plan.plan_id;
    let transfer_entries = plan.transfer;
    let delete = plan.delete;
    let tracker = Arc::new(transfer::LocalChangeTracker::default());
    let mut files_done = 0usize;
    let mut bytes_done = 0u64;
    let files_total = transfer_entries.len();
    let bytes_total = transfer_entries.iter().map(|e| e.size_bytes).sum::<u64>();

    runtime.emit_progress(
        TtSyncProgressEvent {
            direction: TtSyncDirection::Pull,
            phase: SyncPhase::Downloading,
            files_done,
            files_total,
            bytes_done,
            bytes_total,
            current_path: None,
        },
        origin,
    );

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

        let stream = response
            .bytes_stream()
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error));
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

                if transfer::should_emit_progress(files_done, files_total) {
                    runtime.emit_progress(
                        TtSyncProgressEvent {
                            direction: TtSyncDirection::Pull,
                            phase: SyncPhase::Downloading,
                            files_done,
                            files_total,
                            bytes_done,
                            bytes_total,
                            current_path: Some(progress.path),
                        },
                        origin,
                    );
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
        let download_concurrency = transfer::tt_sync_transfer_concurrency();
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

                    if transfer::should_emit_progress(files_done, files_total) {
                        runtime.emit_progress(
                            TtSyncProgressEvent {
                                direction: TtSyncDirection::Pull,
                                phase: SyncPhase::Downloading,
                                files_done,
                                files_total,
                                bytes_done,
                                bytes_total,
                                current_path: Some(joined.path),
                            },
                            origin,
                        );
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
    runtime.emit_progress(
        TtSyncProgressEvent {
            direction: TtSyncDirection::Pull,
            phase: SyncPhase::Deleting,
            files_done: 0,
            files_total: delete_total,
            bytes_done: 0,
            bytes_total: 0,
            current_path: None,
        },
        origin,
    );

    let mut files_deleted = 0usize;
    for sync_path in delete {
        let full_path = transfer::resolve_to_local(&runtime.sync_root, &sync_path);
        if let Err(error) = tokio::fs::remove_file(&full_path).await {
            return Err(SyncExecutionFailure::new(
                DomainError::InternalError(error.to_string()),
                tracker.summary(),
            ));
        }

        files_deleted += 1;
        tracker.record_delete();
        if transfer::should_emit_progress(files_deleted, delete_total) {
            runtime.emit_progress(
                TtSyncProgressEvent {
                    direction: TtSyncDirection::Pull,
                    phase: SyncPhase::Deleting,
                    files_done: files_deleted,
                    files_total: delete_total,
                    bytes_done: 0,
                    bytes_total: 0,
                    current_path: Some(sync_path.to_string()),
                },
                origin,
            );
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
    api: SyncHttpClient,
    sync_root: std::path::PathBuf,
    session_token: SessionToken,
    plan_id: PlanId,
    entry: ManifestEntryV2,
    tracker: Arc<transfer::LocalChangeTracker>,
) {
    join_set.spawn(async move {
        download_one(&api, &sync_root, &session_token, &plan_id, entry, tracker).await
    });
}

async fn download_one(
    api: &SyncHttpClient,
    sync_root: &std::path::Path,
    session_token: &SessionToken,
    plan_id: &PlanId,
    entry: ManifestEntryV2,
    tracker: Arc<transfer::LocalChangeTracker>,
) -> Result<DownloadResult, DomainError> {
    let full_path = transfer::resolve_to_local(sync_root, &entry.path);

    let response = api
        .download_file(session_token, plan_id, &entry.path)
        .await?;
    let stream = response
        .bytes_stream()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error));
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
