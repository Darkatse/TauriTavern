use std::sync::Arc;

use async_compression::tokio::bufread::ZstdEncoder;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::task::JoinSet;
use tokio_util::io::ReaderStream;

use ttsync_contract::manifest::ManifestEntryV2;
use ttsync_contract::peer::DeviceId;
use ttsync_contract::plan::{PlanId, SyncPlan};
use ttsync_contract::session::SessionToken;
use ttsync_contract::sync::{SyncMode, SyncPhase};

use crate::domain::errors::DomainError;
use crate::domain::models::tt_sync::{TtSyncCompletedEvent, TtSyncDirection, TtSyncProgressEvent};
use crate::infrastructure::tt_sync::bundle::{FEATURE_BUNDLE_V1, FEATURE_ZSTD_V1, copy_exact, write_u32_be};
use crate::infrastructure::tt_sync::fs::scan_manifest;
use crate::infrastructure::tt_sync::runtime::TtSyncRuntime;
use crate::infrastructure::tt_sync::transfer;
use crate::infrastructure::tt_sync::v2_api::TtSyncV2Api;

pub async fn push_to_server(
    runtime: Arc<TtSyncRuntime>,
    server_device_id: &DeviceId,
    mode: SyncMode,
) -> Result<TtSyncCompletedEvent, DomainError> {
    let mut server = runtime.get_paired_server(server_device_id).await?;
    let identity = runtime.store.load_or_create_identity().await?;

    let api = TtSyncV2Api::new(server.base_url.clone(), server.spki_sha256.clone())?;
    let features = api.status_features().await.unwrap_or_default();
    let prefer_bundle = features.iter().any(|f| f == FEATURE_BUNDLE_V1);
    let allow_zstd = prefer_bundle && features.iter().any(|f| f == FEATURE_ZSTD_V1);

    let session = api
        .open_session(&identity.device_id, &identity.ed25519_seed)
        .await?;

    server.permissions = session.granted_permissions;
    runtime.upsert_paired_server(server.clone()).await?;

    if !server.permissions.write {
        return Err(DomainError::AuthenticationError(
            "TT-Sync server does not grant write permission".to_string(),
        ));
    }
    if mode == SyncMode::Mirror && !server.permissions.mirror_delete {
        return Err(DomainError::AuthenticationError(
            "TT-Sync server does not grant mirror_delete permission".to_string(),
        ));
    }

    runtime.emit_progress(TtSyncProgressEvent {
        direction: TtSyncDirection::Push,
        phase: SyncPhase::Scanning,
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        current_path: None,
    })?;

    let source_manifest = scan_manifest(runtime.sync_root.clone()).await?;

    runtime.emit_progress(TtSyncProgressEvent {
        direction: TtSyncDirection::Push,
        phase: SyncPhase::Diffing,
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        current_path: None,
    })?;

    let plan = api
        .push_plan(&session.session_token, mode, source_manifest)
        .await?;

    let plan_files_total = plan.files_total;
    let plan_bytes_total = plan.bytes_total;
    let files_deleted = if mode == SyncMode::Mirror {
        plan.delete.len()
    } else {
        0
    };

    apply_push_plan(
        &runtime,
        api,
        &session.session_token,
        plan,
        mode,
        prefer_bundle,
        allow_zstd,
    )
    .await?;

    let mut updated = server;
    updated.last_sync_ms = Some(transfer::now_ms());
    runtime.upsert_paired_server(updated).await?;

    Ok(TtSyncCompletedEvent {
        direction: TtSyncDirection::Push,
        files_total: plan_files_total,
        bytes_total: plan_bytes_total,
        files_deleted,
    })
}

async fn apply_push_plan(
    runtime: &TtSyncRuntime,
    api: TtSyncV2Api,
    session_token: &SessionToken,
    plan: SyncPlan,
    mode: SyncMode,
    prefer_bundle: bool,
    allow_zstd: bool,
) -> Result<(), DomainError> {
    let plan_id = plan.plan_id;
    let transfer_entries = plan.transfer;
    let delete = plan.delete;
    let mut files_done = 0usize;
    let mut bytes_done = 0u64;
    let files_total = transfer_entries.len();
    let bytes_total = transfer_entries.iter().map(|e| e.size_bytes).sum::<u64>();

    runtime.emit_progress(TtSyncProgressEvent {
        direction: TtSyncDirection::Push,
        phase: SyncPhase::Uploading,
        files_done,
        files_total,
        bytes_done,
        bytes_total,
        current_path: None,
    })?;

    if prefer_bundle && !transfer_entries.is_empty() {
        let (progress_tx, mut progress_rx) =
            tokio::sync::mpsc::unbounded_channel::<BundleProgress>();

        let (reader, writer) = tokio::io::duplex(64 * 1024);
        let writer_task = tokio::spawn(write_bundle_upload(
            runtime.sync_root.clone(),
            transfer_entries,
            writer,
            progress_tx,
        ));

        let reader: Box<dyn tokio::io::AsyncRead + Send + Unpin> = if allow_zstd {
            Box::new(ZstdEncoder::new(BufReader::new(reader)))
        } else {
            Box::new(reader)
        };

        let stream = ReaderStream::with_capacity(reader, 64 * 1024);
        let body = reqwest::Body::wrap_stream(stream);

        let mut upload_fut = Box::pin(api.upload_bundle(session_token, &plan_id, body, allow_zstd));
        let upload_result = loop {
            tokio::select! {
                result = &mut upload_fut => break result,
                Some(progress) = progress_rx.recv() => {
                    files_done += 1;
                    bytes_done += progress.size_bytes;

                    if transfer::should_emit_progress(files_done, files_total) {
                        runtime.emit_progress(TtSyncProgressEvent {
                            direction: TtSyncDirection::Push,
                            phase: SyncPhase::Uploading,
                            files_done,
                            files_total,
                            bytes_done,
                            bytes_total,
                            current_path: Some(progress.path),
                        })?;
                    }
                }
            }
        };

        let writer_result = writer_task
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        while let Ok(progress) = progress_rx.try_recv() {
            files_done += 1;
            bytes_done += progress.size_bytes;

            if transfer::should_emit_progress(files_done, files_total) {
                runtime.emit_progress(TtSyncProgressEvent {
                    direction: TtSyncDirection::Push,
                    phase: SyncPhase::Uploading,
                    files_done,
                    files_total,
                    bytes_done,
                    bytes_total,
                    current_path: Some(progress.path),
                })?;
            }
        }

        upload_result?;
        writer_result?;
    } else {
        let upload_concurrency = transfer::tt_sync_transfer_concurrency();
        let mut join_set = JoinSet::new();
        let mut upload_iter = transfer_entries.into_iter();
        let mut in_flight = 0usize;

        while in_flight < upload_concurrency {
            let Some(entry) = upload_iter.next() else {
                break;
            };

            spawn_upload_task(
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
                .ok_or_else(|| DomainError::InternalError("Upload join set ended early".to_string()))?
                .map_err(|error| DomainError::InternalError(error.to_string()))??;

            in_flight -= 1;
            files_done += 1;
            bytes_done += joined.size_bytes;

            if transfer::should_emit_progress(files_done, files_total) {
                runtime.emit_progress(TtSyncProgressEvent {
                    direction: TtSyncDirection::Push,
                    phase: SyncPhase::Uploading,
                    files_done,
                    files_total,
                    bytes_done,
                    bytes_total,
                    current_path: Some(joined.path),
                })?;
            }

            if let Some(entry) = upload_iter.next() {
                spawn_upload_task(
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

    if mode == SyncMode::Mirror && !delete.is_empty() {
        let delete_total = delete.len();
        runtime.emit_progress(TtSyncProgressEvent {
            direction: TtSyncDirection::Push,
            phase: SyncPhase::Deleting,
            files_done: 0,
            files_total: delete_total,
            bytes_done: 0,
            bytes_total: 0,
            current_path: None,
        })?;
    }

    let commit = api.commit(session_token, &plan_id).await?;
    if !commit.ok {
        return Err(DomainError::InternalError(
            "TT-Sync commit returned ok=false".to_string(),
        ));
    }

    if mode == SyncMode::Mirror && !delete.is_empty() {
        let delete_total = delete.len();
        runtime.emit_progress(TtSyncProgressEvent {
            direction: TtSyncDirection::Push,
            phase: SyncPhase::Deleting,
            files_done: delete_total,
            files_total: delete_total,
            bytes_done: 0,
            bytes_total: 0,
            current_path: None,
        })?;
    }

    Ok(())
}

struct BundleProgress {
    path: String,
    size_bytes: u64,
}

async fn write_bundle_upload(
    sync_root: std::path::PathBuf,
    transfer: Vec<ManifestEntryV2>,
    mut out: tokio::io::DuplexStream,
    progress: tokio::sync::mpsc::UnboundedSender<BundleProgress>,
) -> Result<(), DomainError> {
    for entry in transfer {
        let path_bytes = entry.path.as_str().as_bytes();
        let path_len = u32::try_from(path_bytes.len())
            .map_err(|_| DomainError::InvalidData("Bundle path is too long to encode".to_string()))?;

        write_u32_be(&mut out, path_len).await?;
        out.write_all(path_bytes)
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        let full_path = transfer::resolve_to_local(&sync_root, &entry.path);
        let mut file = tokio::fs::File::open(&full_path)
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        copy_exact(&mut file, &mut out, entry.size_bytes).await?;
        let _ = progress.send(BundleProgress {
            path: entry.path.to_string(),
            size_bytes: entry.size_bytes,
        });
    }

    write_u32_be(&mut out, 0).await?;
    Ok(())
}

struct UploadResult {
    path: String,
    size_bytes: u64,
}

fn spawn_upload_task(
    join_set: &mut JoinSet<Result<UploadResult, DomainError>>,
    api: TtSyncV2Api,
    sync_root: std::path::PathBuf,
    session_token: SessionToken,
    plan_id: PlanId,
    entry: ManifestEntryV2,
) {
    join_set
        .spawn(async move { upload_one(&api, &sync_root, &session_token, &plan_id, entry).await });
}

async fn upload_one(
    api: &TtSyncV2Api,
    sync_root: &std::path::Path,
    session_token: &SessionToken,
    plan_id: &PlanId,
    entry: ManifestEntryV2,
) -> Result<UploadResult, DomainError> {
    let full_path = transfer::resolve_to_local(sync_root, &entry.path);
    let file = tokio::fs::File::open(&full_path)
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    let stream = ReaderStream::with_capacity(file, 64 * 1024);
    let body = reqwest::Body::wrap_stream(stream);
    api.upload_file(session_token, plan_id, &entry.path, body)
        .await?;

    Ok(UploadResult {
        path: entry.path.to_string(),
        size_bytes: entry.size_bytes,
    })
}
