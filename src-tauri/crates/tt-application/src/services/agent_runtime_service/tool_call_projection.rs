//! Non-authoritative live projections of the two workspace edit tools.
//! Canonical arguments continue through the existing final-response path.

use std::collections::BTreeMap;

use tokio::sync::watch;
use tt_domain::models::agent::AgentInvocationExitPolicy;
use tt_domain::models::tool::ToolId;

use crate::services::agent_model_gateway::AgentToolCallDelta;
use crate::services::agent_tools::{WORKSPACE_APPLY_PATCH, WORKSPACE_WRITE_FILE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelAttemptGeneration {
    pub round: usize,
    pub attempt: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AgentRunLiveCallKey {
    pub invocation_id: String,
    pub tool_call_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCallProjection {
    WriteFile {
        path: String,
        content: String,
    },
    ApplyPatch {
        path: String,
        old_string: String,
        new_string: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunLiveCall {
    pub generation: ModelAttemptGeneration,
    pub invocation_exit_policy: AgentInvocationExitPolicy,
    pub projection: ToolCallProjection,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentRunLiveProjection {
    pub calls: BTreeMap<AgentRunLiveCallKey, AgentRunLiveCall>,
}

pub(super) struct ToolCallProjector {
    invocation_id: String,
    invocation_exit_policy: AgentInvocationExitPolicy,
    generation: ModelAttemptGeneration,
    sender: watch::Sender<AgentRunLiveProjection>,
    calls: BTreeMap<usize, IncrementalToolCall>,
}

impl ToolCallProjector {
    pub(super) fn new(
        invocation_id: impl Into<String>,
        invocation_exit_policy: AgentInvocationExitPolicy,
        round: usize,
        attempt: usize,
        sender: watch::Sender<AgentRunLiveProjection>,
    ) -> Self {
        Self {
            invocation_id: invocation_id.into(),
            invocation_exit_policy,
            generation: ModelAttemptGeneration { round, attempt },
            sender,
            calls: BTreeMap::new(),
        }
    }

    pub(super) fn observe(&mut self, delta: AgentToolCallDelta) {
        let Some(kind) = ProjectionKind::for_tool(&delta.tool_id) else {
            return;
        };
        let parsed = {
            let call = self
                .calls
                .entry(delta.tool_call_index)
                .or_insert_with(|| IncrementalToolCall::new(kind));
            if call.disabled {
                return;
            }
            match call.scanner.push_fragment(&delta.arguments_fragment) {
                Ok(suffix) if !suffix.is_empty() => Ok(Some(suffix)),
                Ok(_) => Ok(None),
                Err(()) => {
                    call.disabled = true;
                    Err(())
                }
            }
        };
        let suffix = match parsed {
            Ok(Some(parsed)) => parsed,
            Ok(None) => return,
            Err(()) => {
                self.remove_generation_call(delta.tool_call_index);
                return;
            }
        };

        let key = self.key(delta.tool_call_index);
        let generation = self.generation;
        let invocation_exit_policy = self.invocation_exit_policy;
        self.sender.send_if_modified(|state| {
            let call = state.calls.entry(key).or_insert_with(|| AgentRunLiveCall {
                generation,
                invocation_exit_policy,
                projection: ToolCallProjection::empty(kind),
            });
            call.projection.append(suffix);
            true
        });
    }

    pub(super) fn clear(&self) {
        let invocation_id = self.invocation_id.as_str();
        let generation = self.generation;
        self.sender.send_if_modified(|state| {
            let before = state.calls.len();
            state.calls.retain(|key, call| {
                key.invocation_id != invocation_id || call.generation != generation
            });
            state.calls.len() != before
        });
    }

    fn key(&self, tool_call_index: usize) -> AgentRunLiveCallKey {
        AgentRunLiveCallKey {
            invocation_id: self.invocation_id.clone(),
            tool_call_index,
        }
    }

    fn remove_generation_call(&self, tool_call_index: usize) {
        let key = self.key(tool_call_index);
        let generation = self.generation;
        self.sender.send_if_modified(|state| {
            if state.calls.get(&key).map(|call| call.generation) != Some(generation) {
                return false;
            }
            state.calls.remove(&key);
            true
        });
    }
}

pub(super) fn remove_live_tool_call(
    sender: &watch::Sender<AgentRunLiveProjection>,
    invocation_id: &str,
    tool_call_index: usize,
) {
    let key = AgentRunLiveCallKey {
        invocation_id: invocation_id.to_string(),
        tool_call_index,
    };
    sender.send_if_modified(|state| state.calls.remove(&key).is_some());
}

pub(super) fn clear_live_invocation(
    sender: &watch::Sender<AgentRunLiveProjection>,
    invocation_id: &str,
) {
    sender.send_if_modified(|state| {
        let before = state.calls.len();
        state
            .calls
            .retain(|key, _| key.invocation_id != invocation_id);
        state.calls.len() != before
    });
}

struct IncrementalToolCall {
    scanner: TopLevelStringScanner,
    disabled: bool,
}

impl IncrementalToolCall {
    fn new(kind: ProjectionKind) -> Self {
        Self {
            scanner: TopLevelStringScanner::new(kind),
            disabled: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ProjectionKind {
    WriteFile,
    ApplyPatch,
}

impl ProjectionKind {
    fn for_tool(tool_id: &ToolId) -> Option<Self> {
        if !tool_id.is_builtin() {
            return None;
        }
        match tool_id.native_name() {
            WORKSPACE_WRITE_FILE => Some(Self::WriteFile),
            WORKSPACE_APPLY_PATCH => Some(Self::ApplyPatch),
            _ => None,
        }
    }

    fn selected_field(self, key: &str) -> Option<ProjectionField> {
        match (self, key) {
            (Self::WriteFile | Self::ApplyPatch, "path") => Some(ProjectionField::Path),
            (Self::WriteFile, "content") => Some(ProjectionField::Content),
            (Self::ApplyPatch, "old_string") => Some(ProjectionField::OldString),
            (Self::ApplyPatch, "new_string") => Some(ProjectionField::NewString),
            _ => None,
        }
    }
}

impl ToolCallProjection {
    fn empty(kind: ProjectionKind) -> Self {
        match kind {
            ProjectionKind::WriteFile => Self::WriteFile {
                path: String::new(),
                content: String::new(),
            },
            ProjectionKind::ApplyPatch => Self::ApplyPatch {
                path: String::new(),
                old_string: String::new(),
                new_string: String::new(),
            },
        }
    }

    fn append(&mut self, suffix: ProjectionSuffix) {
        match self {
            Self::WriteFile { path, content } => {
                path.push_str(&suffix.path);
                content.push_str(&suffix.content);
            }
            Self::ApplyPatch {
                path,
                old_string,
                new_string,
            } => {
                path.push_str(&suffix.path);
                old_string.push_str(&suffix.old_string);
                new_string.push_str(&suffix.new_string);
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ProjectionField {
    Path,
    Content,
    OldString,
    NewString,
}

#[derive(Default)]
struct ProjectionSuffix {
    path: String,
    content: String,
    old_string: String,
    new_string: String,
}

impl ProjectionSuffix {
    fn push(&mut self, field: ProjectionField, value: char) {
        match field {
            ProjectionField::Path => self.path.push(value),
            ProjectionField::Content => self.content.push(value),
            ProjectionField::OldString => self.old_string.push(value),
            ProjectionField::NewString => self.new_string.push(value),
        }
    }

    fn is_empty(&self) -> bool {
        self.path.is_empty()
            && self.content.is_empty()
            && self.old_string.is_empty()
            && self.new_string.is_empty()
    }
}

/// Decodes selected strings once while retaining only JSON parser state between fragments.
struct TopLevelStringScanner {
    kind: ProjectionKind,
    phase: ScannerPhase,
    escape: EscapeState,
    key: String,
}

impl TopLevelStringScanner {
    fn new(kind: ProjectionKind) -> Self {
        Self {
            kind,
            phase: ScannerPhase::Start,
            escape: EscapeState::None,
            key: String::new(),
        }
    }

    fn push_fragment(&mut self, fragment: &str) -> Result<ProjectionSuffix, ()> {
        let mut suffix = ProjectionSuffix::default();
        for value in fragment.chars() {
            self.push(value, &mut suffix)?;
        }
        Ok(suffix)
    }

    fn push(&mut self, value: char, suffix: &mut ProjectionSuffix) -> Result<(), ()> {
        match self.phase {
            ScannerPhase::Start if json_whitespace(value) => Ok(()),
            ScannerPhase::Start if value == '{' => {
                self.phase = ScannerPhase::KeyOrEnd;
                Ok(())
            }
            ScannerPhase::KeyOrEnd if json_whitespace(value) => Ok(()),
            ScannerPhase::KeyOrEnd if value == '}' => {
                self.phase = ScannerPhase::Done;
                Ok(())
            }
            ScannerPhase::KeyOrEnd if value == '"' => {
                self.key.clear();
                self.phase = ScannerPhase::String(StringTarget::Key);
                Ok(())
            }
            ScannerPhase::Colon if json_whitespace(value) => Ok(()),
            ScannerPhase::Colon if value == ':' => {
                self.phase = ScannerPhase::Value;
                Ok(())
            }
            ScannerPhase::Value if json_whitespace(value) => Ok(()),
            ScannerPhase::Value if value == '"' => {
                self.phase = ScannerPhase::String(
                    self.kind
                        .selected_field(&self.key)
                        .map(StringTarget::Selected)
                        .unwrap_or(StringTarget::Ignored),
                );
                Ok(())
            }
            ScannerPhase::Value if self.kind.selected_field(&self.key).is_some() => Err(()),
            ScannerPhase::Value if matches!(value, '{' | '[' | '}') => Err(()),
            ScannerPhase::Value => {
                self.phase = ScannerPhase::Primitive;
                Ok(())
            }
            ScannerPhase::Primitive if value == ',' => {
                self.phase = ScannerPhase::KeyOrEnd;
                Ok(())
            }
            ScannerPhase::Primitive if value == '}' => {
                self.phase = ScannerPhase::Done;
                Ok(())
            }
            ScannerPhase::Primitive => Ok(()),
            ScannerPhase::CommaOrEnd if json_whitespace(value) => Ok(()),
            ScannerPhase::CommaOrEnd if value == ',' => {
                self.phase = ScannerPhase::KeyOrEnd;
                Ok(())
            }
            ScannerPhase::CommaOrEnd if value == '}' => {
                self.phase = ScannerPhase::Done;
                Ok(())
            }
            ScannerPhase::String(target) => self.push_string(value, target, suffix),
            ScannerPhase::Done if json_whitespace(value) => Ok(()),
            _ => Err(()),
        }
    }

    fn push_string(
        &mut self,
        value: char,
        target: StringTarget,
        suffix: &mut ProjectionSuffix,
    ) -> Result<(), ()> {
        match self.escape {
            EscapeState::None if value == '"' => {
                self.phase = match target {
                    StringTarget::Key => ScannerPhase::Colon,
                    StringTarget::Ignored | StringTarget::Selected(_) => ScannerPhase::CommaOrEnd,
                };
                Ok(())
            }
            EscapeState::None if value == '\\' => {
                self.escape = EscapeState::Escaped;
                Ok(())
            }
            EscapeState::None if value < '\u{20}' => Err(()),
            EscapeState::None => {
                self.append_string_char(target, value, suffix);
                Ok(())
            }
            EscapeState::Escaped => match value {
                '"' | '\\' | '/' => {
                    self.escape = EscapeState::None;
                    self.append_string_char(target, value, suffix);
                    Ok(())
                }
                'b' | 'f' | 'n' | 'r' | 't' => {
                    self.escape = EscapeState::None;
                    let decoded = match value {
                        'b' => '\u{8}',
                        'f' => '\u{c}',
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        _ => unreachable!(),
                    };
                    self.append_string_char(target, decoded, suffix);
                    Ok(())
                }
                'u' => {
                    self.escape = EscapeState::Unicode {
                        value: 0,
                        digits: 0,
                    };
                    Ok(())
                }
                _ => Err(()),
            },
            EscapeState::Unicode {
                value: code,
                digits,
            } => {
                let code = push_hex_digit(code, value)?;
                if digits < 3 {
                    self.escape = EscapeState::Unicode {
                        value: code,
                        digits: digits + 1,
                    };
                    return Ok(());
                }
                if (0xD800..=0xDBFF).contains(&code) {
                    self.escape = EscapeState::LowSurrogateBackslash { high: code };
                    return Ok(());
                }
                if (0xDC00..=0xDFFF).contains(&code) {
                    return Err(());
                }
                self.escape = EscapeState::None;
                self.append_string_char(target, char::from_u32(u32::from(code)).ok_or(())?, suffix);
                Ok(())
            }
            EscapeState::LowSurrogateBackslash { high } if value == '\\' => {
                self.escape = EscapeState::LowSurrogateU { high };
                Ok(())
            }
            EscapeState::LowSurrogateU { high } if value == 'u' => {
                self.escape = EscapeState::LowUnicode {
                    high,
                    value: 0,
                    digits: 0,
                };
                Ok(())
            }
            EscapeState::LowUnicode {
                high,
                value: code,
                digits,
            } => {
                let code = push_hex_digit(code, value)?;
                if digits < 3 {
                    self.escape = EscapeState::LowUnicode {
                        high,
                        value: code,
                        digits: digits + 1,
                    };
                    return Ok(());
                }
                if !(0xDC00..=0xDFFF).contains(&code) {
                    return Err(());
                }
                let scalar =
                    0x10000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(code) - 0xDC00);
                self.escape = EscapeState::None;
                self.append_string_char(target, char::from_u32(scalar).ok_or(())?, suffix);
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn append_string_char(
        &mut self,
        target: StringTarget,
        value: char,
        suffix: &mut ProjectionSuffix,
    ) {
        match target {
            StringTarget::Key => self.key.push(value),
            StringTarget::Selected(field) => suffix.push(field, value),
            StringTarget::Ignored => {}
        }
    }
}

#[derive(Clone, Copy)]
enum ScannerPhase {
    Start,
    KeyOrEnd,
    String(StringTarget),
    Colon,
    Value,
    Primitive,
    CommaOrEnd,
    Done,
}

#[derive(Clone, Copy)]
enum StringTarget {
    Key,
    Selected(ProjectionField),
    Ignored,
}

#[derive(Clone, Copy)]
enum EscapeState {
    None,
    Escaped,
    Unicode { value: u16, digits: u8 },
    LowSurrogateBackslash { high: u16 },
    LowSurrogateU { high: u16 },
    LowUnicode { high: u16, value: u16, digits: u8 },
}

fn push_hex_digit(value: u16, digit: char) -> Result<u16, ()> {
    Ok((value << 4) | u16::try_from(digit.to_digit(16).ok_or(())?).map_err(|_| ())?)
}

fn json_whitespace(value: char) -> bool {
    matches!(value, ' ' | '\n' | '\r' | '\t')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_is_invariant_across_fragment_boundaries() {
        assert_every_split(
            WORKSPACE_WRITE_FILE,
            r#"{"content":"line\nquote:\" slash:\\ emoji:\uD83D\uDE00","path":"output/你好.md","mode":"replace"}"#,
            ToolCallProjection::WriteFile {
                path: "output/你好.md".to_string(),
                content: "line\nquote:\" slash:\\ emoji:😀".to_string(),
            },
        );
        assert_every_split(
            WORKSPACE_APPLY_PATCH,
            r#"{"new_string":"after","replace_all":false,"old_string":"before","path":"output/a.md"}"#,
            ToolCallProjection::ApplyPatch {
                path: "output/a.md".to_string(),
                old_string: "before".to_string(),
                new_string: "after".to_string(),
            },
        );
    }

    #[test]
    fn unsupported_shape_removes_only_that_preview() {
        let (sender, receiver) = watch::channel(AgentRunLiveProjection::default());
        let mut projector = ToolCallProjector::new(
            "inv_a",
            AgentInvocationExitPolicy::RunFinishAllowed,
            1,
            1,
            sender,
        );
        let tool_id = ToolId::builtin(WORKSPACE_WRITE_FILE).unwrap();
        projector.observe(AgentToolCallDelta {
            tool_call_index: 0,
            tool_id: tool_id.clone(),
            arguments_fragment: r#"{"path":"output/a.md","content":"visible","extra":"#.to_string(),
        });
        assert_eq!(receiver.borrow().calls.len(), 1);

        projector.observe(AgentToolCallDelta {
            tool_call_index: 0,
            tool_id,
            arguments_fragment: r#"{"nested":true}}"#.to_string(),
        });
        assert!(receiver.borrow().calls.is_empty());
    }

    #[test]
    fn cleanup_is_scoped_to_one_attempt_or_durable_call() {
        let (sender, receiver) = watch::channel(AgentRunLiveProjection::default());
        let tool_id = ToolId::builtin(WORKSPACE_WRITE_FILE).unwrap();
        let mut first = ToolCallProjector::new(
            "inv_a",
            AgentInvocationExitPolicy::RunFinishAllowed,
            1,
            1,
            sender.clone(),
        );
        let mut sibling = ToolCallProjector::new(
            "inv_b",
            AgentInvocationExitPolicy::TaskReturnRequired,
            1,
            1,
            sender.clone(),
        );
        for projector in [&mut first, &mut sibling] {
            projector.observe(AgentToolCallDelta {
                tool_call_index: 0,
                tool_id: tool_id.clone(),
                arguments_fragment: r#"{"content":"body"}"#.to_string(),
            });
        }

        first.clear();
        assert_eq!(receiver.borrow().calls.len(), 1);
        assert!(
            receiver
                .borrow()
                .calls
                .keys()
                .all(|key| key.invocation_id == "inv_b")
        );

        let mut retry = ToolCallProjector::new(
            "inv_a",
            AgentInvocationExitPolicy::RunFinishAllowed,
            1,
            2,
            sender.clone(),
        );
        retry.observe(AgentToolCallDelta {
            tool_call_index: 0,
            tool_id,
            arguments_fragment: r#"{"content":"retry"}"#.to_string(),
        });
        first.clear();
        let state = receiver.borrow();
        assert_eq!(state.calls.len(), 2);
        assert!(
            state.calls.iter().any(|(key, call)| {
                key.invocation_id == "inv_a" && call.generation.attempt == 2
            })
        );
        drop(state);

        remove_live_tool_call(&sender, "inv_b", 0);
        assert_eq!(receiver.borrow().calls.len(), 1);
        remove_live_tool_call(&sender, "inv_a", 0);
        assert!(receiver.borrow().calls.is_empty());
    }

    fn assert_every_split(tool_name: &str, arguments: &str, expected: ToolCallProjection) {
        for split in (0..=arguments.len()).filter(|split| arguments.is_char_boundary(*split)) {
            let (sender, receiver) = watch::channel(AgentRunLiveProjection::default());
            let mut projector = ToolCallProjector::new(
                "inv",
                AgentInvocationExitPolicy::RunFinishAllowed,
                2,
                3,
                sender,
            );
            let tool_id = ToolId::builtin(tool_name).unwrap();
            for fragment in [&arguments[..split], &arguments[split..]] {
                if !fragment.is_empty() {
                    projector.observe(AgentToolCallDelta {
                        tool_call_index: 0,
                        tool_id: tool_id.clone(),
                        arguments_fragment: fragment.to_string(),
                    });
                }
            }
            let state = receiver.borrow();
            let call = state.calls.values().next().expect("projection missing");
            assert_eq!(call.projection, expected, "split at byte {split}");
            assert_eq!(
                call.generation,
                ModelAttemptGeneration {
                    round: 2,
                    attempt: 3
                }
            );
        }
    }
}
