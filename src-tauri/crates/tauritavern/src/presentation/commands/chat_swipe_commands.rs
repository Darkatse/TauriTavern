use std::sync::Arc;

use serde::Serialize;
use tauri::{Manager, Resource, ResourceId, State, Webview};
use tokio::sync::Mutex;
use tt_application::dto::chat_history_dto::ChatHistoryLocator;
use tt_ports::repositories::chat_payload_commit_repository::{
    ChatSwipeSource, ColdSwipeCommitSource,
};

use super::chat_commands::ChatByteResource;
use crate::app::AppState;
use crate::presentation::errors::CommandError;

const CHUNK_BYTES: usize = 4 * 1024 * 1024;

struct SwipeSourceResource(Arc<dyn ChatSwipeSource>);
impl Resource for SwipeSourceResource {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenColdChatResult {
    source_id: ResourceId,
    reader_id: ResourceId,
}

pub(super) fn commit_source(
    webview: &Webview,
    id: ResourceId,
) -> Result<ColdSwipeCommitSource, CommandError> {
    let resource = webview
        .resources_table()
        .get::<SwipeSourceResource>(id)
        .map_err(|_| {
            CommandError::BadRequest("Cold swipe source expired; reopen the chat".into())
        })?;
    Ok(ColdSwipeCommitSource {
        id,
        source: resource.0.clone(),
    })
}

#[tauri::command]
pub async fn open_cold_chat(
    target: ChatHistoryLocator,
    allow_not_found: bool,
    webview: Webview,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Option<OpenColdChatResult>, CommandError> {
    let Some(source) = app_state
        .services
        .chat_payload_commit_service
        .open_swipe_source(target, allow_not_found)
        .await?
    else {
        return Ok(None);
    };
    let source_id = webview
        .resources_table()
        .add(SwipeSourceResource(source.clone()));
    let reader = source.projection(source_id);
    let reader_id = webview.resources_table().add(ChatByteResource {
        reader: Mutex::new(reader),
        chunk_bytes: CHUNK_BYTES,
    });
    Ok(Some(OpenColdChatResult {
        source_id,
        reader_id,
    }))
}

#[tauri::command]
pub async fn open_cold_swipe_record(
    source_id: ResourceId,
    record: usize,
    webview: Webview,
) -> Result<ResourceId, CommandError> {
    let source = commit_source(&webview, source_id)?;
    let reader = source
        .source
        .record(record)
        .await
        .map_err(tt_application::errors::ApplicationError::from)?;
    Ok(webview.resources_table().add(ChatByteResource {
        reader: Mutex::new(reader),
        chunk_bytes: CHUNK_BYTES,
    }))
}

/// A page reload cannot await JS cleanup. In-flight operations retain their own Arc.
pub(crate) fn close_page_chat_resources(webview: &Webview) {
    let mut table = webview.resources_table();
    let ids: Vec<_> = table
        .names()
        .map(|(rid, _)| rid)
        .filter(|rid| {
            table.get::<SwipeSourceResource>(*rid).is_ok()
                || table.get::<ChatByteResource>(*rid).is_ok()
        })
        .collect();
    for rid in ids {
        let _ = table.close(rid);
    }
}
