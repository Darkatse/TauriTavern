use std::borrow::Cow;
use std::collections::HashMap;

use async_trait::async_trait;
use tt_domain::errors::DomainError;
use tt_domain::frozen_macros::{FrozenMacros, MAX_EXPANDED_TEXT_BYTES};
use tt_domain::models::agent::{AgentChatRef, AgentRun};
use tt_domain::text_lines::TextLineSelection;
use tt_ports::repositories::chat_repository::{ChatMessageReadItem, ChatRepository};
use tt_ports::workspace_shell::{
    ChatMessageOutcome, ChatMessageRead, ChatMessageSource, MessageRange, SCRIPT_MAX_MESSAGE_BYTES,
    SCRIPT_MAX_MESSAGES_PER_CALL,
};

use super::{role_as_str, visible_total_messages};

/// Read-only access to the current run's character chat, for JavaScript scripts.
///
/// Group chats and non-chat runs report [`ChatMessageOutcome::Unsupported`]; only
/// character chats are supported in this version.
pub(crate) struct CharacterChatMessageSource {
    run: AgentRun,
    chat_repository: std::sync::Arc<dyn ChatRepository>,
    macros: std::sync::Arc<FrozenMacros>,
}

impl CharacterChatMessageSource {
    pub(crate) fn new(
        run: AgentRun,
        chat_repository: std::sync::Arc<dyn ChatRepository>,
        macros: std::sync::Arc<FrozenMacros>,
    ) -> Self {
        Self {
            run,
            chat_repository,
            macros,
        }
    }
}

#[async_trait]
impl ChatMessageSource for CharacterChatMessageSource {
    async fn read(
        &self,
        requests: &[(usize, Option<MessageRange>)],
    ) -> Result<ChatMessageOutcome, DomainError> {
        if requests.is_empty() {
            // The JS surface rejects this before dispatch; a direct port caller that
            // sends nothing has broken the request contract, so fail loudly rather
            // than reporting a capability that is in fact available.
            return Err(DomainError::InvalidData(
                "chat read requires at least one message index".into(),
            ));
        }
        if requests.len() > SCRIPT_MAX_MESSAGES_PER_CALL {
            return Err(DomainError::InvalidData(format!(
                "chat read accepts at most {SCRIPT_MAX_MESSAGES_PER_CALL} message indexes per call"
            )));
        }

        // A Session run has no chat target; treat it like any other unsupported
        // capability instead of failing the call.
        let Ok(target) = self.run.chat_target() else {
            return Ok(ChatMessageOutcome::Unsupported);
        };
        let (character_id, file_name) = match &target.chat_ref {
            AgentChatRef::Character {
                character_id,
                file_name,
            } => (character_id.clone(), file_name.clone()),
            AgentChatRef::Group { .. } => return Ok(ChatMessageOutcome::Unsupported),
        };

        // The port contract is to read the run's frozen input view. A run without
        // a frozen count has no bounded view, so exposing the live chat length
        // would read past the run's own input. Only deserialization of records
        // predating `input_message_count` reaches here — every run created by the
        // application sets it — so report the capability as absent rather than
        // guess a bound. Checked before the scan so the scan is skipped entirely.
        if target.input_message_count.is_none() {
            return Ok(ChatMessageOutcome::Unsupported);
        }

        let indices = requests.iter().map(|(index, _)| *index).collect::<Vec<_>>();
        // A single call reads every requested index in one shared file scan.
        let read = match self
            .chat_repository
            .read_character_chat_messages(&character_id, &file_name, &indices)
            .await
        {
            Ok(read) => read,
            Err(DomainError::NotFound(_)) => return Ok(ChatMessageOutcome::ChatNotFound),
            Err(error) => return Err(error),
        };

        let total_messages = visible_total_messages(&self.run, read.total_messages)
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;

        // Render before selecting, exactly as `chat.read_messages` does. A render
        // failure is a real error about the chat content, so it propagates instead
        // of being reported as a bad line range the caller could fix.
        let mut messages = read.messages;
        for message in &mut messages {
            if let Cow::Owned(rendered) =
                self.macros.render(&message.text, MAX_EXPANDED_TEXT_BYTES)?
            {
                message.text = rendered;
            }
        }

        Ok(resolve_messages(requests, messages, total_messages))
    }
}

/// Turn a scanned message set into an outcome for the requested indexes.
///
/// Pure: takes the already-rendered messages and the frozen visible total, so the
/// lookup, duplicate, and per-message limit rules are testable without a live
/// repository.
fn resolve_messages(
    requests: &[(usize, Option<MessageRange>)],
    messages: Vec<ChatMessageReadItem>,
    total_messages: usize,
) -> ChatMessageOutcome {
    if let Some((index, _)) = requests.iter().find(|(index, _)| *index >= total_messages) {
        return ChatMessageOutcome::MessageNotFound {
            index: *index,
            total_messages,
        };
    }

    let by_index = messages
        .into_iter()
        .map(|message| (message.index, message))
        .collect::<HashMap<_, _>>();

    let mut rendered = Vec::with_capacity(requests.len());
    for (index, range) in requests {
        // Look up without removing so a repeated index is served each time
        // instead of being reported as missing on its second occurrence.
        let Some(message) = by_index.get(index) else {
            return ChatMessageOutcome::MessageNotFound {
                index: *index,
                total_messages,
            };
        };
        rendered.push(match render_message(message, *range) {
            Ok(read) => read,
            Err(RenderFailure::TooLarge { total_bytes }) => {
                return ChatMessageOutcome::MessageTooLarge {
                    index: *index,
                    total_bytes,
                    max_bytes: SCRIPT_MAX_MESSAGE_BYTES,
                };
            }
            Err(RenderFailure::InvalidRange(message)) => {
                return ChatMessageOutcome::InvalidRange {
                    index: *index,
                    message,
                };
            }
        });
    }

    ChatMessageOutcome::Found {
        total_messages,
        messages: rendered,
    }
}

#[derive(Debug)]
enum RenderFailure {
    TooLarge { total_bytes: usize },
    InvalidRange(String),
}

/// Select the requested line window of one already macro-rendered message.
///
/// Split from the source so the pure text/limit logic is testable without a
/// live repository.
fn render_message(
    message: &ChatMessageReadItem,
    range: Option<MessageRange>,
) -> Result<ChatMessageRead, RenderFailure> {
    let text = message.text.as_str();
    let total_bytes = text.len();
    // The whole-message budget only gates a read that asks for the entire text.
    // A caller that supplies a line range is explicitly paginating a message it
    // already knows is large, so the selected window is bounded by
    // `TextLineSelection::select_bytes` below and must not be rejected here;
    // otherwise `MessageTooLarge` would be a dead end with no way to proceed.
    if range.is_none() && total_bytes > SCRIPT_MAX_MESSAGE_BYTES {
        return Err(RenderFailure::TooLarge { total_bytes });
    }

    let (start_line, line_count) = match range {
        Some(range) => (range.start_line, range.line_count),
        None => (1, None),
    };
    let selection =
        TextLineSelection::select_bytes(text, start_line, line_count, SCRIPT_MAX_MESSAGE_BYTES)
            .map_err(|error| RenderFailure::InvalidRange(error.to_string()))?;

    let ref_id = format!(
        "chat:current#{}:L{}-L{}",
        message.index, selection.start_line, selection.end_line
    );

    // A caller-supplied start line is deliberate pagination, not truncation, so
    // `TextLineSelection::truncated()` is the wrong signal here: it also reports
    // `start_line > 1`. What the script needs to know is whether content it asked
    // for was withheld — the byte budget ending the window before the requested
    // last line, or clipping a single line.
    let preview =
        selection.line_truncated || selection.end_line < selection.requested_end_line(line_count);

    Ok(ChatMessageRead {
        index: message.index,
        role: role_as_str(message.role),
        name: message.name.clone(),
        send_date: message.send_date.clone(),
        text: selection.content,
        ref_id,
        start_line: selection.start_line,
        end_line: selection.end_line,
        total_lines: selection.total_lines,
        total_bytes,
        preview,
    })
}

#[cfg(test)]
mod tests {
    use tt_ports::repositories::chat_repository::{ChatMessageReadItem, ChatMessageRole};
    use tt_ports::workspace_shell::{ChatMessageOutcome, MessageRange, SCRIPT_MAX_MESSAGE_BYTES};

    use super::{RenderFailure, render_message, resolve_messages};

    fn message(index: usize, role: ChatMessageRole, text: String) -> ChatMessageReadItem {
        ChatMessageReadItem {
            index,
            role,
            name: Some("Tester".into()),
            send_date: Some("2024-01-01T00:00:00Z".into()),
            text,
        }
    }

    fn render(
        message: &ChatMessageReadItem,
        range: Option<MessageRange>,
    ) -> Result<super::ChatMessageRead, RenderFailure> {
        render_message(message, range)
    }

    /// A single-line message past the script-visible byte budget.
    fn oversized(extra: usize) -> String {
        "x".repeat(SCRIPT_MAX_MESSAGE_BYTES + extra)
    }

    fn found(outcome: ChatMessageOutcome) -> (usize, Vec<super::ChatMessageRead>) {
        match outcome {
            ChatMessageOutcome::Found {
                total_messages,
                messages,
            } => (total_messages, messages),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn whole_message_is_returned_with_a_reference_spanning_all_lines() {
        let rendered = render(
            &message(4, ChatMessageRole::Assistant, "one\ntwo\nthree".into()),
            None,
        )
        .expect("renders");
        assert_eq!(rendered.index, 4);
        assert_eq!(rendered.role, "assistant");
        assert_eq!(rendered.text, "one\ntwo\nthree");
        assert_eq!(rendered.ref_id, "chat:current#4:L1-L3");
        assert_eq!(rendered.start_line, 1);
        assert_eq!(rendered.end_line, 3);
        assert_eq!(rendered.total_lines, 3);
        assert_eq!(rendered.total_bytes, 13);
        assert!(!rendered.preview);
    }

    #[test]
    fn totals_are_measured_in_bytes_rather_than_characters() {
        // The budget and the reported totals share one unit, so a script can size
        // its next window from them.
        let text = "你好世界";
        let rendered =
            render(&message(2, ChatMessageRole::User, text.into()), None).expect("renders");
        assert_eq!(rendered.total_bytes, text.len());
        assert_ne!(rendered.total_bytes, text.chars().count());
    }

    #[test]
    fn a_message_within_the_byte_budget_is_not_a_preview() {
        // Guard the common case so `preview` cannot become a constant true.
        let body = (1..=900)
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let rendered =
            render(&message(0, ChatMessageRole::Assistant, body), None).expect("renders");
        assert_eq!(rendered.total_lines, 900);
        assert_eq!(rendered.end_line, 900);
        assert!(!rendered.preview);
    }

    #[test]
    fn the_byte_budget_marks_a_requested_window_it_cut_as_a_preview() {
        // Only the byte budget can end a script read early, and the script has to
        // see it even when the window it asked for is what got cut.
        let half = SCRIPT_MAX_MESSAGE_BYTES / 2 + 10;
        let body = format!("{}\n{}", "x".repeat(half), "y".repeat(half));
        let rendered = render(
            &message(0, ChatMessageRole::Assistant, body),
            Some(MessageRange {
                start_line: 1,
                line_count: Some(2),
            }),
        )
        .expect("renders");
        assert_eq!(rendered.end_line, 1);
        assert_eq!(rendered.total_lines, 2);
        assert!(rendered.preview);
    }

    #[test]
    fn an_explicit_start_line_is_not_a_preview() {
        // Reading from line 5 to the end of a 20-line message returns everything
        // the caller asked for. `TextLineSelection::truncated()` reports this as
        // truncated because `start_line > 1`; `preview` must not.
        let body = (1..=20)
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let rendered = render(
            &message(0, ChatMessageRole::User, body),
            Some(MessageRange {
                start_line: 5,
                line_count: None,
            }),
        )
        .expect("renders");
        assert_eq!(rendered.start_line, 5);
        assert_eq!(rendered.end_line, 20);
        assert!(!rendered.preview);
    }

    #[test]
    fn a_caller_bounded_window_is_not_a_preview() {
        // A `lineCount` window that stops short of the end is the caller's own
        // bound, so it is pagination rather than withheld content.
        let body = (1..=50)
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let rendered = render(
            &message(0, ChatMessageRole::User, body),
            Some(MessageRange {
                start_line: 1,
                line_count: Some(10),
            }),
        )
        .expect("renders");
        assert_eq!(rendered.end_line, 10);
        assert_eq!(rendered.total_lines, 50);
        assert!(!rendered.preview);
    }

    #[test]
    fn a_clipped_single_line_is_a_preview() {
        // A ranged read of a single line longer than the byte budget is cut
        // mid-line with `start_line == 1`, so only `line_truncated` can mark it.
        // (The unbounded read of the same message is rejected as too large before
        // selection runs, which is why this needs an explicit range.)
        let rendered = render(
            &message(0, ChatMessageRole::Assistant, oversized(10)),
            Some(MessageRange {
                start_line: 1,
                line_count: None,
            }),
        )
        .expect("renders");
        assert_eq!(rendered.end_line, 1);
        assert_eq!(rendered.text.len(), SCRIPT_MAX_MESSAGE_BYTES);
        assert!(rendered.preview);
    }

    #[test]
    fn an_explicit_range_selects_only_that_window() {
        let rendered = render(
            &message(0, ChatMessageRole::User, "a\nb\nc\nd".into()),
            Some(MessageRange {
                start_line: 2,
                line_count: Some(2),
            }),
        )
        .expect("renders");
        assert_eq!(rendered.text, "b\nc");
        assert_eq!(rendered.ref_id, "chat:current#0:L2-L3");
        assert_eq!(rendered.total_lines, 4);
        assert!(!rendered.preview);
    }

    #[test]
    fn a_message_over_the_byte_budget_is_rejected() {
        let failure = render(&message(2, ChatMessageRole::Assistant, oversized(1)), None)
            .expect_err("rejected");
        match failure {
            RenderFailure::TooLarge { total_bytes } => {
                assert_eq!(total_bytes, SCRIPT_MAX_MESSAGE_BYTES + 1);
            }
            RenderFailure::InvalidRange(message) => panic!("unexpected range failure: {message}"),
        }
    }

    #[test]
    fn an_oversized_message_can_still_be_read_by_range() {
        // A message past the whole-message budget stays readable when the caller
        // paginates; the reported total lets the script size the next window.
        let mut text = "a".repeat(SCRIPT_MAX_MESSAGE_BYTES).into_bytes();
        text.push(b'\n');
        text.extend_from_slice(b"tail");
        let oversized = String::from_utf8(text).unwrap();
        let rendered = render(
            &message(2, ChatMessageRole::Assistant, oversized),
            Some(MessageRange {
                start_line: 2,
                line_count: Some(1),
            }),
        )
        .expect("a ranged read of an oversized message succeeds");
        assert_eq!(rendered.text, "tail");
        assert_eq!(rendered.start_line, 2);
        assert_eq!(rendered.end_line, 2);
        assert_eq!(rendered.total_lines, 2);
        assert_eq!(rendered.total_bytes, SCRIPT_MAX_MESSAGE_BYTES + 5);
        assert!(!rendered.preview);
    }

    #[test]
    fn a_range_beyond_the_message_is_rejected() {
        let failure = render(
            &message(0, ChatMessageRole::User, "only line".into()),
            Some(MessageRange {
                start_line: 99,
                line_count: None,
            }),
        )
        .expect_err("rejected");
        assert!(matches!(failure, RenderFailure::InvalidRange(_)));
    }

    #[test]
    fn a_repeated_index_is_served_for_every_occurrence() {
        let messages = vec![
            message(0, ChatMessageRole::User, "first".into()),
            message(1, ChatMessageRole::Assistant, "second".into()),
        ];
        let requests = vec![(1, None), (0, None), (1, None)];
        let (total, rendered) = found(resolve_messages(&requests, messages, 2));
        assert_eq!(total, 2);
        let texts = rendered.iter().map(|m| m.text.as_str()).collect::<Vec<_>>();
        assert_eq!(texts, ["second", "first", "second"]);
    }

    #[test]
    fn an_index_at_or_beyond_the_frozen_total_is_not_found() {
        let messages = vec![message(0, ChatMessageRole::User, "only".into())];
        // The frozen total is 1, so index 1 is out of range even though a larger
        // payload may exist on disk.
        let outcome = resolve_messages(&[(1, None)], messages, 1);
        match outcome {
            ChatMessageOutcome::MessageNotFound {
                index,
                total_messages,
            } => {
                assert_eq!(index, 1);
                assert_eq!(total_messages, 1);
            }
            other => panic!("expected MessageNotFound, got {other:?}"),
        }
    }

    #[test]
    fn an_oversized_requested_message_reports_its_measured_size() {
        let messages = vec![message(3, ChatMessageRole::Assistant, oversized(7))];
        let outcome = resolve_messages(&[(3, None)], messages, 4);
        match outcome {
            ChatMessageOutcome::MessageTooLarge {
                index,
                total_bytes,
                max_bytes,
            } => {
                assert_eq!(index, 3);
                assert_eq!(total_bytes, SCRIPT_MAX_MESSAGE_BYTES + 7);
                assert_eq!(max_bytes, SCRIPT_MAX_MESSAGE_BYTES);
            }
            other => panic!("expected MessageTooLarge, got {other:?}"),
        }
    }
}
