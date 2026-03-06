use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Client;
use tokio::io::AsyncWriteExt;
use tokio::task::JoinSet;
use url::Url;

use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::{
    LanSyncDiffPlan, LanSyncSyncCompletedEvent, LanSyncSyncMode, LanSyncSyncPhase,
    LanSyncSyncProgressEvent,
};
use crate::infrastructure::lan_sync::crypto::sign_request;
use crate::infrastructure::lan_sync::manifest::scan_manifest;
use crate::infrastructure::lan_sync::paths::resolve_relative_path;
use crate::infrastructure::lan_sync::runtime::LanSyncRuntime;

pub async fn merge_sync_from_device(
    runtime: Arc<LanSyncRuntime>,
    http_client: &Client,
    device_id: &str,
) -> Result<LanSyncSyncCompletedEvent, DomainError> {
    let config = runtime.store.load_or_create_config().await?;
    let sync_mode_override = runtime.get_sync_mode_override().await;
    let sync_mode = sync_mode_override.unwrap_or(config.sync_mode);

    let peer = runtime.get_paired_device(device_id).await?;
    let address = peer.last_known_address.clone().ok_or_else(|| {
        DomainError::InvalidData(format!("Paired device address is missing: {}", device_id))
    })?;

    let identity = runtime.store.load_or_create_identity().await?;

    runtime.emit_sync_progress(LanSyncSyncProgressEvent {
        phase: LanSyncSyncPhase::Scanning,
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        current_path: None,
    })?;

    let target_manifest = scan_manifest(runtime.sync_root.clone()).await?;

    runtime.emit_sync_progress(LanSyncSyncProgressEvent {
        phase: LanSyncSyncPhase::Diffing,
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        current_path: None,
    })?;

    let plan_url = build_source_url(&address, &["v1", "sync", "plan"])?;
    let body = serde_json::to_vec(&target_manifest)
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    let signature = sign_request(peer.pair_secret.as_bytes(), "POST", "/v1/sync/plan", &body);

    let response = http_client
        .post(plan_url)
        .header("X-TT-Device-Id", identity.device_id.clone())
        .header("X-TT-Signature", signature)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        return Err(DomainError::AuthenticationError(format!(
            "Sync plan failed ({}): {}",
            status, body
        )));
    }

    let plan = response
        .json::<LanSyncDiffPlan>()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    let delete_total = plan.delete.len();
    let mut files_done = 0usize;
    let mut bytes_done = 0u64;
    let files_total = plan.files_total;
    let bytes_total = plan.bytes_total;

    runtime.emit_sync_progress(LanSyncSyncProgressEvent {
        phase: LanSyncSyncPhase::Downloading,
        files_done,
        files_total,
        bytes_done,
        bytes_total,
        current_path: None,
    })?;

    let download_concurrency = default_download_concurrency();
    let mut join_set = JoinSet::new();
    let mut download_iter = plan.download.into_iter();
    let mut in_flight = 0usize;

    while in_flight < download_concurrency {
        let Some(entry) = download_iter.next() else {
            break;
        };

        spawn_download_task(
            &mut join_set,
            http_client.clone(),
            address.clone(),
            peer.pair_secret.clone(),
            identity.device_id.clone(),
            runtime.sync_root.clone(),
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

        if should_emit_progress(files_done, files_total) {
            runtime.emit_sync_progress(LanSyncSyncProgressEvent {
                phase: LanSyncSyncPhase::Downloading,
                files_done,
                files_total,
                bytes_done,
                bytes_total,
                current_path: Some(joined.relative_path),
            })?;
        }

        if let Some(entry) = download_iter.next() {
            spawn_download_task(
                &mut join_set,
                http_client.clone(),
                address.clone(),
                peer.pair_secret.clone(),
                identity.device_id.clone(),
                runtime.sync_root.clone(),
                entry,
            );
            in_flight += 1;
        }
    }

    let mut files_deleted = 0usize;
    if sync_mode == LanSyncSyncMode::Mirror && delete_total > 0 {
        runtime.emit_sync_progress(LanSyncSyncProgressEvent {
            phase: LanSyncSyncPhase::Deleting,
            files_done: 0,
            files_total: delete_total,
            bytes_done: 0,
            bytes_total: 0,
            current_path: None,
        })?;

        for relative_path in plan.delete {
            let full_path = resolve_relative_path(&runtime.sync_root, &relative_path)?;
            tokio::fs::remove_file(&full_path)
                .await
                .map_err(|error| DomainError::InternalError(error.to_string()))?;

            files_deleted += 1;
            if should_emit_progress(files_deleted, delete_total) {
                runtime.emit_sync_progress(LanSyncSyncProgressEvent {
                    phase: LanSyncSyncPhase::Deleting,
                    files_done: files_deleted,
                    files_total: delete_total,
                    bytes_done: 0,
                    bytes_total: 0,
                    current_path: Some(relative_path),
                })?;
            }
        }
    }

    let mut updated_peer = peer;
    updated_peer.last_sync_ms = Some(now_ms());
    runtime.upsert_paired_device(updated_peer).await?;

    Ok(LanSyncSyncCompletedEvent {
        files_total,
        bytes_total,
        files_deleted,
    })
}

struct DownloadResult {
    relative_path: String,
    size_bytes: u64,
}

fn spawn_download_task(
    join_set: &mut JoinSet<Result<DownloadResult, DomainError>>,
    http_client: Client,
    address: String,
    pair_secret: String,
    device_id: String,
    sync_root: std::path::PathBuf,
    entry: crate::domain::models::lan_sync::LanSyncManifestEntry,
) {
    join_set.spawn(async move {
        download_one(
            &http_client,
            &address,
            &pair_secret,
            &device_id,
            &sync_root,
            entry,
        )
        .await
    });
}

async fn download_one(
    http_client: &Client,
    address: &str,
    pair_secret: &str,
    device_id: &str,
    sync_root: &std::path::Path,
    entry: crate::domain::models::lan_sync::LanSyncManifestEntry,
) -> Result<DownloadResult, DomainError> {
    let full_path = resolve_relative_path(sync_root, &entry.relative_path)?;

    if let Some(parent) = full_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
    }

    let file_url = build_source_file_url(address, &entry.relative_path)?;
    let canonical_path = format!("/v1/sync/file/{}", entry.relative_path);
    let signature = sign_request(pair_secret.as_bytes(), "GET", &canonical_path, &[]);

    let mut response = http_client
        .get(file_url)
        .header("X-TT-Device-Id", device_id.to_string())
        .header("X-TT-Signature", signature)
        .send()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        return Err(DomainError::InternalError(format!(
            "File download failed ({}): {}",
            status, body
        )));
    }

    let tmp_path = full_path.with_extension("ttsync.tmp");
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp_path)
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
    }

    file.flush()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    drop(file);

    match tokio::fs::rename(&tmp_path, &full_path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            match tokio::fs::remove_file(&full_path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(DomainError::InternalError(error.to_string())),
            }

            tokio::fs::rename(&tmp_path, &full_path)
                .await
                .map_err(|error| DomainError::InternalError(error.to_string()))?;
        }
        Err(error) => return Err(DomainError::InternalError(error.to_string())),
    }

    set_file_modified_ms(&full_path, entry.modified_ms)?;

    Ok(DownloadResult {
        relative_path: entry.relative_path,
        size_bytes: entry.size_bytes,
    })
}

fn build_source_url(address: &str, segments: &[&str]) -> Result<Url, DomainError> {
    let mut url =
        Url::parse(address).map_err(|error| DomainError::InvalidData(error.to_string()))?;

    {
        let mut path_segments = url
            .path_segments_mut()
            .map_err(|_| DomainError::InvalidData("Invalid source address".to_string()))?;
        path_segments.clear();
        path_segments.extend(segments.iter().copied());
    }

    Ok(url)
}

fn build_source_file_url(address: &str, relative_path: &str) -> Result<Url, DomainError> {
    let mut url =
        Url::parse(address).map_err(|error| DomainError::InvalidData(error.to_string()))?;

    {
        let mut path_segments = url
            .path_segments_mut()
            .map_err(|_| DomainError::InvalidData("Invalid source address".to_string()))?;
        path_segments.clear();
        path_segments.push("v1");
        path_segments.push("sync");
        path_segments.push("file");
        for segment in relative_path.split('/') {
            path_segments.push(segment);
        }
    }

    Ok(url)
}

fn set_file_modified_ms(path: &std::path::Path, modified_ms: u64) -> Result<(), DomainError> {
    let secs = (modified_ms / 1000) as i64;
    let nanos = ((modified_ms % 1000) * 1_000_000) as u32;
    let mtime = filetime::FileTime::from_unix_time(secs, nanos);

    filetime::set_file_mtime(path, mtime)
        .map_err(|error| DomainError::InternalError(error.to_string()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn default_download_concurrency() -> usize {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        2
    } else {
        4
    }
}

fn should_emit_progress(files_done: usize, files_total: usize) -> bool {
    files_done == files_total || files_done == 1 || files_done % 10 == 0
}
