use crate::errors::ApplicationError;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::OnceCell;
use tt_domain::errors::DomainError;
use tt_domain::frozen_macros::FrozenMacros;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{AgentChatRef, AgentRun, AgentRunTarget, WorkspacePath};
use tt_domain::models::skill::SkillIndexEntry;
use tt_ports::repositories::chat_repository::ChatRepository;
use tt_ports::repositories::skill_repository::SkillRepository;
use tt_ports::workspace_fs::{
    WorkspaceAppendResult, WorkspaceDirectoryEntry, WorkspaceEntryKind, WorkspaceFs,
    WorkspaceMetadata, WorkspaceWriteGuard,
};

mod skills;

pub(crate) const AGENT_TOOL_RESULTS_ROOT: &str = "tool-results";
pub(crate) const SKILLS_ROOT: &str = "skills";
const CHAT_FILE: &str = "chat.json";
const FLOORS_ROOT: &str = "floors";
/// The mount table: top-level names served only by mounts, never by the Run directory.
/// All are read-only.
const MOUNT_ROOTS: [&str; 3] = [SKILLS_ROOT, CHAT_FILE, FLOORS_ROOT];

pub(crate) fn task_result_summary_path(workspace_key: &str) -> Result<WorkspacePath, DomainError> {
    WorkspacePath::parse(format!("summaries/{workspace_key}-result.md"))
}

pub(crate) fn workspace_path_is_under_any_root(path: &WorkspacePath, roots: &[String]) -> bool {
    roots
        .iter()
        .any(|root| path_matches_root_or_child(path.as_str(), root))
}

pub(crate) fn format_model_workspace_roots(roots: &[String]) -> String {
    roots
        .iter()
        .map(|root| format!("{root}/"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn format_model_visible_workspace_roots(roots: &[String]) -> String {
    let mut roots = roots.to_vec();
    for root in [AGENT_TOOL_RESULTS_ROOT, SKILLS_ROOT] {
        if !roots.iter().any(|existing| existing == root) {
            roots.push(root.to_string());
        }
    }
    format_model_workspace_roots(&roots)
}

fn path_matches_root_or_child(path: &str, root: &str) -> bool {
    path == root || path_matches_child(path, root)
}

fn path_matches_child(path: &str, root: &str) -> bool {
    path.len() > root.len()
        && path.starts_with(root)
        && path.as_bytes().get(root.len()) == Some(&b'/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_path_is_under_any_root_matches_root_boundary() {
        let roots = vec!["output".to_string()];

        assert!(workspace_path_is_under_any_root(
            &WorkspacePath::parse("output/main.md").unwrap(),
            &roots
        ));
        assert!(workspace_path_is_under_any_root(
            &WorkspacePath::parse("output").unwrap(),
            &roots
        ));
        assert!(!workspace_path_is_under_any_root(
            &WorkspacePath::parse("output_extra/main.md").unwrap(),
            &roots
        ));
    }
}

#[derive(Debug)]
pub(crate) struct WorkspaceAccessPolicy {
    pub(crate) visible_roots: Vec<String>,
    pub(crate) writable_roots: Vec<String>,
}

impl WorkspaceAccessPolicy {
    pub(crate) fn from_profile(profile: &ResolvedAgentProfile) -> Self {
        let mut visible_roots = profile.workspace.visible_roots.clone();
        for root in [AGENT_TOOL_RESULTS_ROOT, SKILLS_ROOT] {
            if !visible_roots.iter().any(|existing| existing == root) {
                visible_roots.push(root.to_string());
            }
        }
        visible_roots.sort();
        visible_roots.dedup();
        let mut writable_roots: Vec<_> = profile
            .workspace
            .writable_roots
            .iter()
            .filter(|root| ![AGENT_TOOL_RESULTS_ROOT, SKILLS_ROOT].contains(&root.as_str()))
            .cloned()
            .collect();
        writable_roots.sort();
        writable_roots.dedup();
        Self {
            visible_roots,
            writable_roots,
        }
    }

    pub(crate) fn ensure_visible(&self, path: &WorkspacePath) -> Result<(), ApplicationError> {
        if self.is_visible(path) {
            return Ok(());
        }

        let value = path.as_str();
        Err(ApplicationError::PermissionDenied(format!(
            "`{value}` is not readable for this task."
        )))
    }

    pub(crate) fn ensure_writable(&self, path: &WorkspacePath) -> Result<(), ApplicationError> {
        if self.is_writable(path) {
            return Ok(());
        }

        let value = path.as_str();
        Err(ApplicationError::PermissionDenied(format!(
            "`{value}` is not writable for this task."
        )))
    }

    /// Mounted paths are visible by construction; an absent mount reads as not found.
    pub(crate) fn is_visible(&self, path: &WorkspacePath) -> bool {
        is_mount_path(path) || workspace_path_is_under_any_root(path, &self.visible_roots)
    }

    pub(crate) fn is_writable(&self, path: &WorkspacePath) -> bool {
        !is_mount_path(path)
            && self
                .writable_roots
                .iter()
                .any(|root| path_matches_child(path.as_str(), root))
    }
}

/// Invocation policy is a view over the Run's shared filesystem, not a copy.
pub(crate) struct ScopedWorkspaceFs {
    inner: Arc<dyn WorkspaceFs>,
    pub(crate) policy: WorkspaceAccessPolicy,
    skill_repository: Option<Arc<dyn SkillRepository>>,
    skill_bindings: Arc<[SkillIndexEntry]>,
    frozen_macros: Arc<FrozenMacros>,
    chat: Option<Arc<ChatMount>>,
    text_mutation: Option<Mutex<WorkspaceTextMutation>>,
}

/// A read-only view mounted at top-level names of the logical tree.
#[derive(Clone, Copy)]
enum Mount<'a> {
    Skills,
    Chat(&'a ChatMount),
}

enum Route<'a> {
    Run,
    Mount(Mount<'a>),
    /// A mount name whose mount this run does not have.
    Missing,
}

/// A Shell call starts from the round's candidate and reports whether it changed.
/// Only the message body can be a candidate; other text files are working notes.
#[derive(Clone)]
pub(crate) struct WorkspaceTextMutation {
    pub(crate) candidate: Option<WorkspacePath>,
    pub(crate) changed: bool,
    message_body: WorkspacePath,
}

impl ScopedWorkspaceFs {
    pub(crate) fn new(inner: Arc<dyn WorkspaceFs>, policy: WorkspaceAccessPolicy) -> Self {
        Self {
            inner,
            policy,
            skill_repository: None,
            skill_bindings: Arc::default(),
            frozen_macros: Arc::default(),
            chat: None,
            text_mutation: None,
        }
    }

    pub(crate) fn with_skills(
        mut self,
        repository: Arc<dyn SkillRepository>,
        bindings: Arc<[SkillIndexEntry]>,
        frozen_macros: Arc<FrozenMacros>,
    ) -> Self {
        self.skill_repository = Some(repository);
        self.skill_bindings = bindings;
        self.frozen_macros = frozen_macros;
        self
    }

    pub(crate) fn with_chat(mut self, chat: Option<Arc<ChatMount>>) -> Self {
        self.chat = chat;
        self
    }

    pub(crate) fn has_chat(&self) -> bool {
        self.chat.is_some()
    }

    /// Chat texts to search from the snapshot, never traversed as files: every floor's
    /// `message.md` when `path` is `None`, otherwise every chat file at or under `path`.
    /// Empty when `path` lies outside the chat mount or the run has no chat.
    pub(crate) async fn chat_texts(
        &self,
        path: Option<&WorkspacePath>,
    ) -> Result<Vec<ChatText<'_>>, DomainError> {
        let Some(path) = path else {
            return match self.chat.as_deref() {
                Some(chat) => chat.texts(None).await,
                None => Ok(Vec::new()),
            };
        };
        self.check(path, false)?;
        match self.route(path) {
            Route::Mount(Mount::Chat(chat)) => chat.texts(Some(path)).await,
            Route::Missing => Err(mounted_path_not_found(path)),
            Route::Run | Route::Mount(Mount::Skills) => Ok(Vec::new()),
        }
    }

    /// The mount serving a top-level mount name, `None` when this run does not have it.
    fn mount(&self, root: &str) -> Option<Mount<'_>> {
        match root {
            SKILLS_ROOT => Some(Mount::Skills),
            CHAT_FILE | FLOORS_ROOT => self.chat.as_deref().map(Mount::Chat),
            _ => None,
        }
    }

    fn route(&self, path: &WorkspacePath) -> Route<'_> {
        let top = path
            .as_str()
            .split('/')
            .next()
            .expect("split yields a segment");
        if MOUNT_ROOTS.contains(&top) {
            self.mount(top).map_or(Route::Missing, Route::Mount)
        } else {
            Route::Run
        }
    }

    pub(crate) fn track_text_mutations(
        mut self,
        message_body: WorkspacePath,
        candidate: Option<WorkspacePath>,
    ) -> Self {
        self.text_mutation = Some(Mutex::new(WorkspaceTextMutation {
            candidate,
            changed: false,
            message_body,
        }));
        self
    }

    fn mutation(&self) -> Option<MutexGuard<'_, WorkspaceTextMutation>> {
        self.text_mutation.as_ref().map(|mutation| {
            mutation
                .lock()
                .expect("workspace text mutation lock poisoned")
        })
    }

    pub(crate) fn text_mutation(&self) -> Option<WorkspaceTextMutation> {
        self.mutation().map(|mutation| mutation.clone())
    }

    fn remember_write(&self, path: &WorkspacePath) {
        if let Some(mut mutation) = self.mutation()
            && mutation.message_body == *path
        {
            mutation.candidate = Some(path.clone());
            mutation.changed = true;
        }
    }

    fn forget_removed(&self, path: &WorkspacePath) {
        if let Some(mut mutation) = self.mutation()
            && mutation.candidate.as_ref().is_some_and(|candidate| {
                path_matches_root_or_child(candidate.as_str(), path.as_str())
            })
        {
            mutation.candidate = None;
            mutation.changed = true;
        }
    }

    fn check(&self, path: &WorkspacePath, write: bool) -> Result<(), DomainError> {
        if if write {
            self.policy.is_writable(path)
        } else {
            self.policy.is_visible(path)
        } {
            Ok(())
        } else {
            Err(DomainError::WorkspaceAccessDenied {
                path: path.as_str().to_owned(),
                operation: if write { "write" } else { "read" },
            })
        }
    }
}

#[async_trait]
impl WorkspaceFs for ScopedWorkspaceFs {
    async fn read_file(
        &self,
        path: &WorkspacePath,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, DomainError> {
        self.check(path, false)?;
        match self.route(path) {
            Route::Run => self.inner.read_file(path, maximum_bytes).await,
            Route::Mount(Mount::Skills) => self.read_skill_file(path, maximum_bytes).await,
            Route::Mount(Mount::Chat(chat)) => chat.read_file(path, maximum_bytes).await,
            Route::Missing => Err(mounted_path_not_found(path)),
        }
    }
    async fn write_file(
        &self,
        path: &WorkspacePath,
        bytes: &[u8],
        guard: WorkspaceWriteGuard,
    ) -> Result<(), DomainError> {
        self.check(path, true)?;
        self.inner.write_file(path, bytes, guard).await?;
        self.remember_write(path);
        Ok(())
    }
    async fn append_file(&self, path: &WorkspacePath, bytes: &[u8]) -> Result<(), DomainError> {
        self.check(path, true)?;
        self.inner.append_file(path, bytes).await?;
        self.remember_write(path);
        Ok(())
    }
    async fn append_text(
        &self,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceAppendResult, DomainError> {
        self.check(path, true)?;
        let result = self.inner.append_text(path, text).await?;
        self.remember_write(path);
        Ok(result)
    }
    async fn metadata(
        &self,
        path: Option<&WorkspacePath>,
    ) -> Result<WorkspaceMetadata, DomainError> {
        match path {
            Some(path) => {
                self.check(path, false)?;
                match self.route(path) {
                    Route::Run => self.inner.metadata(Some(path)).await,
                    Route::Mount(Mount::Skills) => self.skill_metadata(path).await,
                    Route::Mount(Mount::Chat(chat)) => chat.metadata(path).await,
                    Route::Missing => Err(mounted_path_not_found(path)),
                }
            }
            None => Ok(virtual_directory_metadata()),
        }
    }
    async fn read_dir(
        &self,
        path: Option<&WorkspacePath>,
        maximum_entries: usize,
    ) -> Result<Vec<WorkspaceDirectoryEntry>, DomainError> {
        if let Some(path) = path {
            self.check(path, false)?;
            return match self.route(path) {
                Route::Run => self.inner.read_dir(Some(path), maximum_entries).await,
                Route::Mount(Mount::Skills) => self.read_skill_dir(path, maximum_entries).await,
                Route::Mount(Mount::Chat(chat)) => chat.read_dir(path, maximum_entries).await,
                Route::Missing => Err(mounted_path_not_found(path)),
            };
        }
        let mut roots = self.policy.visible_roots.clone();
        for mount_root in MOUNT_ROOTS {
            if self.mount(mount_root).is_some() && !roots.iter().any(|root| root == mount_root) {
                roots.push(mount_root.to_string());
            }
        }
        if roots.len() > maximum_entries {
            return Err(DomainError::InvalidData(format!(
                "Workspace root exceeds {maximum_entries} entries"
            )));
        }
        let mut entries = Vec::new();
        for root in &roots {
            let path = WorkspacePath::parse(root)?;
            let metadata = self.metadata(Some(&path)).await?;
            entries.push(WorkspaceDirectoryEntry { path, metadata });
        }
        entries.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
        Ok(entries)
    }
    async fn create_dir(&self, path: &WorkspacePath, recursive: bool) -> Result<(), DomainError> {
        self.check(path, true)?;
        self.inner.create_dir(path, recursive).await
    }
    async fn remove(&self, path: &WorkspacePath, recursive: bool) -> Result<(), DomainError> {
        self.check(path, true)?;
        self.inner.remove(path, recursive).await?;
        self.forget_removed(path);
        Ok(())
    }
    async fn rename(
        &self,
        source: &WorkspacePath,
        target: &WorkspacePath,
    ) -> Result<(), DomainError> {
        self.check(source, true)?;
        self.check(target, true)?;
        self.inner.rename(source, target).await?;
        if let Some(message_body) = self.text_mutation().map(|mutation| mutation.message_body) {
            let target_is_body = *target == message_body
                && self.inner.metadata(Some(target)).await?.kind == WorkspaceEntryKind::File;
            let mut mutation = self.mutation().expect("text mutation tracking is enabled");
            if target_is_body {
                mutation.candidate = Some(target.clone());
                mutation.changed = true;
            } else if let Some(path) = mutation
                .candidate
                .as_ref()
                .filter(|path| path_matches_root_or_child(path.as_str(), source.as_str()))
            {
                // Move the known candidate with its directory, without scanning the subtree.
                let suffix = &path.as_str()[source.as_str().len()..];
                let moved = WorkspacePath::parse(format!("{}{suffix}", target.as_str()))?;
                mutation.candidate = (moved == message_body).then_some(moved);
                mutation.changed = true;
            }
        }
        Ok(())
    }
    async fn copy_file(
        &self,
        source: &WorkspacePath,
        target: &WorkspacePath,
    ) -> Result<(), DomainError> {
        self.check(source, false)?;
        self.check(target, true)?;
        if matches!(self.route(source), Route::Run) {
            self.inner.copy_file(source, target).await?;
        } else {
            let bytes = self.read_file(source, usize::MAX).await?;
            self.inner
                .write_file(target, &bytes, WorkspaceWriteGuard::Unchecked)
                .await?;
        }
        self.remember_write(target);
        Ok(())
    }
}

/// Paths served by the chat mount, which search reads from its snapshot.
pub(crate) fn is_chat_mount_path(path: &WorkspacePath) -> bool {
    [CHAT_FILE, FLOORS_ROOT]
        .iter()
        .any(|root| path_matches_root_or_child(path.as_str(), root))
}

fn is_mount_path(path: &WorkspacePath) -> bool {
    MOUNT_ROOTS
        .iter()
        .any(|root| path_matches_root_or_child(path.as_str(), root))
}

fn mounted_path_not_found(path: &WorkspacePath) -> DomainError {
    DomainError::NotFound(format!("Workspace path not found: {}", path.as_str()))
}

fn virtual_directory_metadata() -> WorkspaceMetadata {
    WorkspaceMetadata {
        kind: WorkspaceEntryKind::Directory,
        bytes: 0,
        modified: None,
        created: None,
    }
}

/// The current character chat as read-only files at the workspace root:
/// `chat.json` and `floors/NNNNNN/{message.md,meta.json}`.
/// `message.md` is the raw `mes` (no macros, no regex). Floors at or after the run's
/// frozen input count are not part of the view. The chat is read once, on first access,
/// and kept for the holder of this mount; a resumed run reads it again, without checking
/// that earlier floors are unchanged.
pub(crate) struct ChatMount {
    character: String,
    file_name: String,
    stable_chat_id: String,
    input_message_count: Option<usize>,
    repository: Arc<dyn ChatRepository>,
    snapshot: OnceCell<ChatSnapshot>,
}

impl std::fmt::Debug for ChatMount {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChatMount")
            .field("file_name", &self.file_name)
            .field("input_message_count", &self.input_message_count)
            .finish_non_exhaustive()
    }
}

struct ChatSnapshot {
    chat_json: String,
    floors: Vec<ChatFloor>,
}

impl ChatSnapshot {
    /// The chosen files of `floors`, in path order.
    fn floor_texts(
        &self,
        floors: std::ops::Range<usize>,
        message: bool,
        meta: bool,
    ) -> Vec<ChatText<'_>> {
        floors
            .flat_map(|index| {
                let floor = &self.floors[index];
                [
                    (message, "message.md", &floor.message),
                    (meta, "meta.json", &floor.meta),
                ]
                .into_iter()
                .filter(|(included, ..)| *included)
                .map(move |(_, file, text)| ChatText {
                    path: floor_file_path(index, file),
                    text,
                    hidden: floor.hidden,
                })
            })
            .collect()
    }
}

struct ChatFloor {
    message: String,
    meta: String,
    hidden: bool,
}

/// A chat file's text as searched in place, without reading it as a file.
pub(crate) struct ChatText<'a> {
    pub(crate) path: String,
    pub(crate) text: &'a str,
    /// `is_system`: hidden from the prompt by the user, still part of the chat.
    pub(crate) hidden: bool,
}

#[derive(Clone, Copy)]
enum ChatNode {
    ChatJson,
    Floors,
    Floor(usize),
    Message(usize),
    Meta(usize),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatInfo<'a> {
    title: &'a str,
    stable_chat_id: &'a str,
    character: &'a str,
    floor_count: usize,
}

/// SillyTavern field names, as in the chat file.
#[derive(Serialize)]
struct FloorMeta<'a> {
    index: usize,
    name: &'a Value,
    role: &'static str,
    is_system: bool,
    send_date: &'a Value,
    swipe_id: &'a Value,
    swipe_count: Option<usize>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    kind: Option<&'a Value>,
}

impl ChatMount {
    /// Only character chat runs have a chat mount; Agent mode does not run in group chats.
    pub(crate) fn for_run(
        run: &AgentRun,
        repository: Arc<dyn ChatRepository>,
    ) -> Result<Option<Arc<Self>>, DomainError> {
        let AgentRunTarget::Chat(chat) = &run.target else {
            return Ok(None);
        };
        let AgentChatRef::Character {
            character_id,
            file_name,
        } = &chat.chat_ref
        else {
            return Ok(None);
        };
        Ok(Some(Arc::new(Self {
            character: character_id.clone(),
            file_name: file_name.clone(),
            stable_chat_id: chat.stable_chat_id.clone(),
            input_message_count: chat.input_message_count,
            repository,
            snapshot: OnceCell::new(),
        })))
    }

    async fn snapshot(&self) -> Result<&ChatSnapshot, DomainError> {
        self.snapshot.get_or_try_init(|| self.read_snapshot()).await
    }

    async fn read_snapshot(&self) -> Result<ChatSnapshot, DomainError> {
        let payload = self
            .repository
            .get_chat_payload(&self.character, &self.file_name)
            .await?;
        // The first record is the chat header.
        let messages = payload.get(1..).unwrap_or_default();
        // Same bound as the chat tools: only floors before the run's frozen input.
        let floor_count = match self.input_message_count {
            Some(count) if messages.len() < count => {
                return Err(DomainError::InvalidData(format!(
                    "agent.input_history_conflict: run input requires {count} messages, but chat payload has {}",
                    messages.len()
                )));
            }
            Some(count) => count,
            None => messages.len(),
        };
        let floors = messages[..floor_count]
            .iter()
            .enumerate()
            .map(|(index, message)| project_floor(index, message))
            .collect::<Result<Vec<_>, _>>()?;
        let chat_json = to_pretty_json(&ChatInfo {
            title: self
                .file_name
                .strip_suffix(".jsonl")
                .unwrap_or(&self.file_name),
            stable_chat_id: &self.stable_chat_id,
            character: &self.character,
            floor_count,
        })?;
        Ok(ChatSnapshot { chat_json, floors })
    }

    fn node(path: &WorkspacePath) -> Option<ChatNode> {
        let mut parts = path.as_str().split('/');
        let node = match (parts.next(), parts.next(), parts.next()) {
            (Some(CHAT_FILE), None, None) => ChatNode::ChatJson,
            (Some(FLOORS_ROOT), None, None) => ChatNode::Floors,
            (Some(FLOORS_ROOT), Some(floor), file) => {
                let index = parse_floor(floor)?;
                match file {
                    None => ChatNode::Floor(index),
                    Some("message.md") => ChatNode::Message(index),
                    Some("meta.json") => ChatNode::Meta(index),
                    Some(_) => return None,
                }
            }
            _ => return None,
        };
        parts.next().is_none().then_some(node)
    }

    async fn resolve(
        &self,
        path: &WorkspacePath,
    ) -> Result<(&ChatSnapshot, ChatNode), DomainError> {
        let snapshot = self.snapshot().await?;
        let node = Self::node(path).ok_or_else(|| mounted_path_not_found(path))?;
        if let ChatNode::Floor(index) | ChatNode::Message(index) | ChatNode::Meta(index) = node
            && index >= snapshot.floors.len()
        {
            return Err(DomainError::NotFound(format!(
                "Workspace path not found: {}; this chat has {} floors.",
                path.as_str(),
                snapshot.floors.len()
            )));
        }
        Ok((snapshot, node))
    }

    /// Every floor's `message.md` when `path` is `None`, otherwise the files at or under
    /// `path`.
    async fn texts(&self, path: Option<&WorkspacePath>) -> Result<Vec<ChatText<'_>>, DomainError> {
        let Some(path) = path else {
            let snapshot = self.snapshot().await?;
            return Ok(snapshot.floor_texts(0..snapshot.floors.len(), true, false));
        };
        let (snapshot, node) = self.resolve(path).await?;
        Ok(match node {
            ChatNode::ChatJson => vec![ChatText {
                path: CHAT_FILE.to_owned(),
                text: &snapshot.chat_json,
                hidden: false,
            }],
            // A directory search covers messages; meta.json is searched only when named.
            ChatNode::Floors => snapshot.floor_texts(0..snapshot.floors.len(), true, false),
            ChatNode::Floor(index) => snapshot.floor_texts(index..index + 1, true, false),
            ChatNode::Message(index) => snapshot.floor_texts(index..index + 1, true, false),
            ChatNode::Meta(index) => snapshot.floor_texts(index..index + 1, false, true),
        })
    }

    async fn read_file(
        &self,
        path: &WorkspacePath,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, DomainError> {
        let (snapshot, node) = self.resolve(path).await?;
        let text = match node {
            ChatNode::ChatJson => &snapshot.chat_json,
            ChatNode::Message(index) => &snapshot.floors[index].message,
            ChatNode::Meta(index) => &snapshot.floors[index].meta,
            ChatNode::Floors | ChatNode::Floor(_) => {
                return Err(DomainError::workspace_path_is_directory(path.as_str()));
            }
        };
        if text.len() > maximum_bytes {
            return Err(DomainError::InvalidData(format!(
                "Workspace read exceeds {maximum_bytes} bytes: {}",
                path.as_str()
            )));
        }
        Ok(text.as_bytes().to_vec())
    }

    async fn metadata(&self, path: &WorkspacePath) -> Result<WorkspaceMetadata, DomainError> {
        let (snapshot, node) = self.resolve(path).await?;
        Ok(match node {
            ChatNode::ChatJson => file_metadata(&snapshot.chat_json),
            ChatNode::Message(index) => file_metadata(&snapshot.floors[index].message),
            ChatNode::Meta(index) => file_metadata(&snapshot.floors[index].meta),
            ChatNode::Floors | ChatNode::Floor(_) => virtual_directory_metadata(),
        })
    }

    async fn read_dir(
        &self,
        path: &WorkspacePath,
        maximum_entries: usize,
    ) -> Result<Vec<WorkspaceDirectoryEntry>, DomainError> {
        let (snapshot, node) = self.resolve(path).await?;
        let entries = match node {
            ChatNode::Floors => (0..snapshot.floors.len())
                .map(|index| (floor_name(index), virtual_directory_metadata()))
                .collect(),
            ChatNode::Floor(index) => {
                let floor = &snapshot.floors[index];
                vec![
                    ("message.md".to_string(), file_metadata(&floor.message)),
                    ("meta.json".to_string(), file_metadata(&floor.meta)),
                ]
            }
            ChatNode::ChatJson | ChatNode::Message(_) | ChatNode::Meta(_) => {
                return Err(DomainError::file_io(
                    "list",
                    path.as_str(),
                    std::io::Error::from(std::io::ErrorKind::NotADirectory),
                ));
            }
        };
        if entries.len() > maximum_entries {
            return Err(DomainError::InvalidData(format!(
                "Workspace directory exceeds {maximum_entries} entries: {}",
                path.as_str()
            )));
        }
        entries
            .into_iter()
            .map(|(name, metadata)| {
                Ok(WorkspaceDirectoryEntry {
                    path: WorkspacePath::parse(format!("{}/{name}", path.as_str()))?,
                    metadata,
                })
            })
            .collect()
    }
}

fn floor_name(index: usize) -> String {
    format!("{index:06}")
}

fn floor_file_path(index: usize, file: &str) -> String {
    format!("{FLOORS_ROOT}/{}/{file}", floor_name(index))
}

/// Where the chat mount serves a floor's raw message, e.g. `floors/000003/message.md`.
pub(crate) fn floor_message_path(index: usize) -> String {
    floor_file_path(index, "message.md")
}

/// Only the canonical name is a floor: `12` and `+00012` are not `000012`.
fn parse_floor(name: &str) -> Option<usize> {
    let index = name.parse::<usize>().ok()?;
    (floor_name(index) == name).then_some(index)
}

fn project_floor(index: usize, message: &Value) -> Result<ChatFloor, DomainError> {
    static NULL: Value = Value::Null;
    let field = |key: &str| message.get(key).unwrap_or(&NULL);
    let text = message.get("mes").and_then(Value::as_str).ok_or_else(|| {
        DomainError::InvalidData(format!("Chat message {index} has no string `mes` field"))
    })?;
    let kind = message
        .pointer("/extra/type")
        .filter(|value| !value.is_null());
    let role = if message.get("role").and_then(Value::as_str) == Some("tool") {
        "tool"
    } else if field("is_user").as_bool().unwrap_or(false) {
        "user"
    } else if kind.and_then(Value::as_str) == Some("narrator") {
        "system"
    } else {
        "assistant"
    };
    let hidden = field("is_system").as_bool().unwrap_or(false);
    let meta = to_pretty_json(&FloorMeta {
        index,
        name: field("name"),
        role,
        is_system: hidden,
        send_date: field("send_date"),
        swipe_id: field("swipe_id"),
        swipe_count: message
            .get("swipes")
            .and_then(Value::as_array)
            .map(Vec::len),
        kind,
    })?;
    Ok(ChatFloor {
        message: text.to_owned(),
        meta,
        hidden,
    })
}

fn to_pretty_json(value: &impl Serialize) -> Result<String, DomainError> {
    serde_json::to_string_pretty(value)
        .map_err(|error| DomainError::InternalError(format!("Chat mount JSON failed: {error}")))
}

fn file_metadata(text: &str) -> WorkspaceMetadata {
    WorkspaceMetadata {
        kind: WorkspaceEntryKind::File,
        bytes: text.len() as u64,
        modified: None,
        created: None,
    }
}
