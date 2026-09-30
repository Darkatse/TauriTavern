use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use tt_domain::errors::DomainError;
use tt_domain::frozen_macros::{FrozenMacros, MAX_EXPANDED_TEXT_BYTES};
use tt_domain::models::agent::{AgentChatRef, AgentRun};
use tt_domain::text_lines::TextLineSelection;
use tt_ports::repositories::chat_repository::{ChatMessageReadItem, ChatRepository};
use tt_ports::workspace_shell::{
    ChatMessageOutcome, ChatMessageRead, ChatMessageSource, MessageRange, SCRIPT_MAX_CALL_BYTES,
    SCRIPT_MAX_MESSAGE_BYTES, SCRIPT_MESSAGE_ENTRY_BYTES,
};

use super::{chat_input_count_missing, role_as_str, visible_total_messages};

/// Read-only access to the current run's character chat, for JavaScript scripts.
///
/// Only constructed for a character-chat run: the dispatcher withholds the source from
/// group chats, non-chat runs, and Profiles that deny `chat.read_messages`, and the
/// script sees each of those as `chat.unsupported`.
pub(crate) struct CharacterChatMessageSource {
    run: AgentRun,
    chat_repository: Arc<dyn ChatRepository>,
    macros: Arc<FrozenMacros>,
}

impl CharacterChatMessageSource {
    pub(crate) fn new(
        run: AgentRun,
        chat_repository: Arc<dyn ChatRepository>,
        macros: Arc<FrozenMacros>,
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
        // The per-call byte budget is the only ceiling, and this is its entry-count
        // form. It is derived from the budget instead of being a separate knob, so
        // raising the budget raises the number of messages that can be read. It is
        // applied before the scan because every requested index costs memory there
        // even when it returns no text: a request this long is malformed, since no
        // message could be returned within the budget it is asking for.
        let max_entries = SCRIPT_MAX_CALL_BYTES / SCRIPT_MESSAGE_ENTRY_BYTES;
        if requests.len() > max_entries {
            return Err(DomainError::InvalidData(format!(
                "chat read accepts at most {max_entries} message indexes per call"
            )));
        }

        // Invariant: this source is only constructed for a run whose chat target is a
        // character chat, which the dispatcher decides from the run target. A group
        // chat, a Session run, and a Profile that withholds `chat.read_messages` all
        // reach the script as `chat.unsupported` because no source is passed at all, so
        // they never arrive here: a non-character target means the construction rule was
        // broken, and reporting the capability as absent would hide that.
        let target = self.run.chat_target()?;
        let (character_id, file_name) = match &target.chat_ref {
            AgentChatRef::Character {
                character_id,
                file_name,
            } => (character_id.clone(), file_name.clone()),
            AgentChatRef::Group { .. } => {
                return Err(DomainError::InvalidData(format!(
                    "agent.chat_target_not_character: run `{}` has no character chat to read",
                    self.run.id
                )));
            }
        };

        // The port contract is to read the run's frozen input view, and the shared
        // helper reports a missing count the same way for every reader. This is only a
        // short-circuit ahead of the scan, so a run that cannot be bounded never pays
        // for one: the constraint itself lives in `visible_total_messages` below, which
        // is what every reader goes through, so a change to the rule must reach there.
        if target.input_message_count.is_none() {
            return Err(chat_input_count_missing(&self.run));
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

        // Already a `DomainError`, so the `agent.input_history_conflict` marker the
        // helper carries reaches the script unchanged.
        let total_messages = visible_total_messages(&self.run, read.total_messages)?;

        let mut messages = read.messages;
        // Index verdicts come first, exactly as `chat.read_messages` orders them: a
        // missing index is a precise answer the script can act on, and a render
        // failure must not replace it with a verdict the caller cannot compare
        // against the request it made.
        let present = messages
            .iter()
            .map(|message| message.index)
            .collect::<HashSet<_>>();
        if let Some(index) = missing_index(requests, &present, total_messages) {
            return Ok(ChatMessageOutcome::MessageNotFound {
                index,
                total_messages,
            });
        }

        // Render before selecting, exactly as `chat.read_messages` does. A render
        // failure is a real error about the chat content, so it propagates instead
        // of being reported as a bad line range the caller could fix.
        for message in &mut messages {
            if let Cow::Owned(rendered) =
                self.macros.render(&message.text, MAX_EXPANDED_TEXT_BYTES)?
            {
                message.text = rendered;
            }
        }

        Ok(resolve_messages(requests, &messages, total_messages))
    }
}

/// First requested index the scanned payload cannot serve, in request order.
///
/// The frozen upper bound and a payload that lacks the index mean the same thing to
/// the script — no message at that index — so both are answered by one verdict.
/// Pure, so the order against macro rendering is testable without a repository.
fn missing_index(
    requests: &[(usize, Option<MessageRange>)],
    present: &HashSet<usize>,
    total_messages: usize,
) -> Option<usize> {
    requests
        .iter()
        .map(|(index, _)| *index)
        .find(|index| *index >= total_messages || !present.contains(index))
}

/// Turn an already-checked message set into an outcome for the requested indexes.
///
/// Pure: takes the rendered messages and the frozen visible total, so the duplicate,
/// range, and over-limit rules are testable without a live repository. Missing
/// indexes are rejected by [`missing_index`] before rendering, so every request that
/// reaches here has a message. The result is all-or-nothing, including the per-call
/// budget: a batch the budget cannot hold is reported whole rather than trimmed, so
/// the script always sees one result per request it made.
fn resolve_messages(
    requests: &[(usize, Option<MessageRange>)],
    messages: &[ChatMessageReadItem],
    total_messages: usize,
) -> ChatMessageOutcome {
    let by_index = messages
        .iter()
        .map(|message| (message.index, message))
        .collect::<HashMap<_, _>>();

    let mut rendered = Vec::with_capacity(requests.len());
    let mut charged = 0_usize;
    for (index, range) in requests {
        // Look up without removing so a repeated index is served each time instead
        // of being reported as missing on its second occurrence.
        let message = *by_index
            .get(index)
            .expect("missing indexes were rejected before rendering");
        let read = match render_message(message, *range) {
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
        };

        // Charge the entry before keeping it: the script heap holds the text and a
        // result object per message, so both are what the call budget bounds.
        let used =
            charged.saturating_add(SCRIPT_MESSAGE_ENTRY_BYTES.saturating_add(read.text.len()));
        if used > SCRIPT_MAX_CALL_BYTES {
            return ChatMessageOutcome::CallTooLarge {
                index: *index,
                used_bytes: used,
                max_bytes: SCRIPT_MAX_CALL_BYTES,
            };
        }
        charged = used;
        rendered.push(read);
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
    // last line, or clipping a single line. The selection carries the requested end
    // itself, so this cannot drift from the window that was actually asked for.
    let preview = selection.line_truncated || selection.end_line < selection.requested_end_line;

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
    use std::collections::HashSet;

    use tt_ports::repositories::chat_repository::{ChatMessageReadItem, ChatMessageRole};
    use tt_ports::workspace_shell::{
        ChatMessageOutcome, MessageRange, SCRIPT_MAX_CALL_BYTES, SCRIPT_MAX_MESSAGE_BYTES,
    };

    use super::{RenderFailure, missing_index, render_message, resolve_messages};

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
        let (total, rendered) = found(resolve_messages(&requests, &messages, 2));
        assert_eq!(total, 2);
        let texts = rendered.iter().map(|m| m.text.as_str()).collect::<Vec<_>>();
        assert_eq!(texts, ["second", "first", "second"]);
    }

    /// Nine single-line messages of 1 MB each: a ranged read of the first eight fits
    /// the per-call budget, and the ninth is what pushes a call over it.
    fn bulk_messages() -> Vec<ChatMessageReadItem> {
        (0..9)
            .map(|index| message(index, ChatMessageRole::Assistant, "x".repeat(1_000_000)))
            .collect()
    }

    fn first_line_requests(count: usize) -> Vec<(usize, Option<MessageRange>)> {
        (0..count)
            .map(|index| {
                (
                    index,
                    Some(MessageRange {
                        start_line: 1,
                        line_count: Some(1),
                    }),
                )
            })
            .collect()
    }

    #[test]
    fn a_call_the_budget_cannot_hold_is_refused_whole() {
        // The per-call budget is what bounds a batch now, so it has to be spent by
        // the messages themselves: eight 1 MB messages fit and the ninth does not,
        // and the verdict names the request that did not fit instead of returning a
        // partial batch the caller would have to match up itself.
        let (total, messages) = found(resolve_messages(
            &first_line_requests(8),
            &bulk_messages(),
            9,
        ));
        assert_eq!(total, 9);
        assert_eq!(messages.len(), 8);

        let outcome = resolve_messages(&first_line_requests(9), &bulk_messages(), 9);
        match outcome {
            ChatMessageOutcome::CallTooLarge {
                index,
                used_bytes,
                max_bytes,
            } => {
                assert_eq!(index, 8);
                assert!(used_bytes > max_bytes);
                assert_eq!(max_bytes, SCRIPT_MAX_CALL_BYTES);
            }
            other => panic!("expected CallTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn an_index_the_payload_cannot_serve_is_not_found_before_rendering() {
        let messages = [message(0, ChatMessageRole::User, "only".into())];
        let present = messages.iter().map(|m| m.index).collect::<HashSet<_>>();

        // The frozen total is 1, so index 1 is out of range even though a larger
        // payload may exist on disk, while a repeated index that is present is not
        // missing at all.
        assert_eq!(missing_index(&[(1, None), (0, None)], &present, 1), Some(1));
        assert_eq!(missing_index(&[(0, None), (0, None)], &present, 1), None);

        // A payload that lacks a requested index inside the frozen bound gets the same
        // verdict, and the first such request in the caller's order is the one named.
        assert_eq!(
            missing_index(&[(0, None), (2, None), (1, None)], &HashSet::new(), 3),
            Some(0)
        );
    }

    #[test]
    fn an_oversized_requested_message_reports_its_measured_size() {
        let messages = vec![message(3, ChatMessageRole::Assistant, oversized(7))];
        let outcome = resolve_messages(&[(3, None)], &messages, 4);
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
