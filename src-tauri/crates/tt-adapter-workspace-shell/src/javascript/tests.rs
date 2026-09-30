use std::sync::Arc;
use std::time::Duration;

use bashkit::{Bash, ExecResult, ExecutionLimits};

use super::Javascript;
use crate::engine::MAX_OUTPUT_BYTES;

async fn execute(command: &str) -> bashkit::Result<ExecResult> {
    let javascript = Arc::new(Javascript::new(Arc::default(), None));
    let mut bash = Bash::builder()
        .builtin("js", javascript.builtin("js"))
        .builtin("node", javascript.builtin("node"))
        .builtin("deno", javascript.builtin("deno"))
        .limits(ExecutionLimits::new().timeout(Duration::from_secs(1)))
        .build();
    let result = bash.exec(command).await;
    javascript.finish().await.unwrap();
    result
}

#[tokio::test]
async fn awaited_module_keeps_output_separate_from_diagnostics() {
    let result = execute(
        r#"
js - draft <<'JS'
import { log } from '@tauritavern/runtime';
const name = await Promise.resolve(process.argv[2]);
log.info('processing', name);
console.log(JSON.stringify({ name }));
JS
"#,
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(result.stdout.as_bytes()).unwrap(),
        serde_json::json!({ "name": "draft" }),
    );
    assert!(result.stderr.text_lossy().contains("processing draft"));
}

#[tokio::test]
async fn file_entry_stops_interpreter_options() {
    let result = execute(
        r#"
mkdir -p /scratch
echo 'console.log(JSON.stringify(process.argv))' > /scratch/-args.mjs
cd /scratch
node -- -args.mjs --help -1 -- '' 'two words'
"#,
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(result.stdout.as_bytes()).unwrap(),
        serde_json::json!([
            "node",
            "/scratch/-args.mjs",
            "--help",
            "-1",
            "--",
            "",
            "two words"
        ]),
    );
}

#[tokio::test]
async fn eval_options_end_at_separator_or_first_operand() {
    for (command, tail, expected) in [
        ("js -e", "-- --help", serde_json::json!(["js", "--help"])),
        (
            "deno eval",
            "alpha --help",
            serde_json::json!(["deno", "alpha", "--help"]),
        ),
    ] {
        let result = execute(&format!(
            "{command} 'console.log(JSON.stringify(process.argv))' {tail}"
        ))
        .await
        .unwrap();
        assert_eq!(result.exit_code, 0, "{command} {tail}: {}", result.stderr);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(result.stdout.as_bytes()).unwrap(),
            expected,
            "{command} {tail}",
        );
    }
    let help = execute("js -e 'throw new Error(\"must not run\")' --help")
        .await
        .unwrap();
    assert_eq!(help.exit_code, 0, "{}", help.stderr);
    assert!(!help.stdout.is_empty());
}

#[tokio::test]
async fn exit_code_rejects_invalid_assignments_before_continuing() {
    for value in ["\"1\"", "1.5", "256"] {
        let result = execute(&format!(
            "js -e 'process.exitCode = {value}; console.log(\"must not run\")'"
        ))
        .await
        .unwrap();
        assert_eq!(result.exit_code, 1, "{value}");
        assert!(result.stdout.is_empty(), "{value}");
        assert!(
            result.stderr.text_lossy().contains("process.exitCode"),
            "{value}"
        );
    }
}

#[tokio::test]
async fn execution_failures_override_exit_code_and_bound_output() {
    let timeout = execute("js -e 'while (true) {}'").await.unwrap_err();
    assert!(
        matches!(timeout, bashkit::Error::ResourceLimit(_)),
        "{timeout}"
    );
    let oversized_result = format!(
        "js -e 'console.log(JSON.stringify({{ text: \"x\".repeat({}) }}))'",
        MAX_OUTPUT_BYTES + 1,
    );
    for (command, reason) in [
        (
            "js -e 'process.exitCode = 7; await Promise.reject(new Error(\"rejected\"))'",
            "rejected",
        ),
        ("js -e 'await new Promise(() => {})'", "Promise"),
        (oversized_result.as_str(), "output"),
    ] {
        let result = execute(command).await.unwrap();
        assert_eq!(result.exit_code, 1, "{command}");
        assert!(result.stdout.is_empty(), "{command}: {}", result.stdout);
        assert!(
            result.stderr.text_lossy().contains(reason),
            "{command}: {}",
            result.stderr,
        );
    }
}

mod chat {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use bashkit::{Bash, ExecResult, ExecutionLimits};
    use tt_domain::errors::DomainError;
    use tt_ports::workspace_shell::{
        ChatMessageOutcome, ChatMessageRead, ChatMessageSource, MessageRange,
    };

    use super::Javascript;

    /// One message the fake chat exposes, in absolute 0-based order.
    struct FakeMessage {
        role: &'static str,
        text: String,
    }

    /// A chat source that records the requests it receives and replays a scripted
    /// outcome.
    ///
    /// It deliberately performs no rendering of its own: the window, limit, and
    /// `ref` rules live in `message_source.rs` and are covered by its pure tests.
    /// Keeping them out of here means the end-to-end tests below assert only that
    /// the JS surface parses arguments and passes them through unchanged, and this
    /// fake never has to track a change in the real render rules.
    #[derive(Default)]
    struct FakeChat {
        messages: Vec<FakeMessage>,
        /// Returned by `read` instead of reading `messages`, for tests that need a
        /// specific failure outcome without reproducing the rule that produces it.
        forced: Option<ChatMessageOutcome>,
        calls: AtomicUsize,
        /// Every request this source received, in order.
        seen: Mutex<Vec<(usize, Option<MessageRange>)>>,
    }

    impl FakeChat {
        fn new(messages: Vec<FakeMessage>) -> Arc<Self> {
            Arc::new(Self {
                messages,
                ..Self::default()
            })
        }

        /// A source whose every call returns `outcome`.
        fn forced(outcome: ChatMessageOutcome) -> Arc<Self> {
            Arc::new(Self {
                forced: Some(outcome),
                ..Self::default()
            })
        }

        fn text(role: &'static str, body: impl Into<String>) -> FakeMessage {
            FakeMessage {
                role,
                text: body.into(),
            }
        }

        fn request_for(&self, position: usize) -> (usize, Option<MessageRange>) {
            self.seen.lock().unwrap()[position]
        }
    }

    #[async_trait]
    impl ChatMessageSource for FakeChat {
        async fn read(
            &self,
            requests: &[(usize, Option<MessageRange>)],
        ) -> Result<ChatMessageOutcome, DomainError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.seen.lock().unwrap().extend_from_slice(requests);
            if let Some(outcome) = &self.forced {
                return Ok(outcome.clone());
            }
            let total = self.messages.len();
            let mut rendered = Vec::with_capacity(requests.len());
            for (index, range) in requests {
                let Some(message) = self.messages.get(*index) else {
                    return Ok(ChatMessageOutcome::MessageNotFound {
                        index: *index,
                        total_messages: total,
                    });
                };
                // The slice mirrors the request mechanically; it is not a second
                // implementation of the line-selection rules.
                let lines = message.text.lines().collect::<Vec<_>>();
                let start_line = range.map_or(1, |range| range.start_line);
                let end_line = match range.and_then(|range| range.line_count) {
                    Some(count) => (start_line - 1 + count).min(lines.len()),
                    None => lines.len(),
                };
                rendered.push(ChatMessageRead {
                    index: *index,
                    role: message.role,
                    name: None,
                    send_date: Some("2024-01-01T00:00:00Z".into()),
                    text: lines[start_line - 1..end_line].join("\n"),
                    ref_id: format!("chat:current#{}:L{}-L{}", index, start_line, end_line),
                    start_line,
                    end_line,
                    total_lines: lines.len(),
                    total_bytes: message.text.len(),
                    preview: false,
                });
            }
            Ok(ChatMessageOutcome::Found {
                total_messages: total,
                messages: rendered,
            })
        }
    }

    /// A chat source that refuses every call, as group chats and Session runs do.
    struct UnsupportedChat;

    #[async_trait]
    impl ChatMessageSource for UnsupportedChat {
        async fn read(
            &self,
            _requests: &[(usize, Option<MessageRange>)],
        ) -> Result<ChatMessageOutcome, DomainError> {
            Ok(ChatMessageOutcome::Unsupported)
        }
    }

    /// A chat source that fails the whole call, as an unusable chat payload or a
    /// macro render failure does.
    struct FailingChat;

    #[async_trait]
    impl ChatMessageSource for FailingChat {
        async fn read(
            &self,
            _requests: &[(usize, Option<MessageRange>)],
        ) -> Result<ChatMessageOutcome, DomainError> {
            Err(DomainError::InvalidData(
                "macro expansion exceeds 16777216 bytes".into(),
            ))
        }
    }

    async fn execute(script: &str, chat: Option<Arc<dyn ChatMessageSource>>) -> ExecResult {
        execute_with_limits(script, chat, |limits| limits)
            .await
            .unwrap()
    }

    /// Run a script under shell limits the test chooses, so the resource budgets a
    /// bridge charges can be exercised without moving large payloads. A spent budget
    /// is fail-closed, so exhausting one surfaces as a shell error here.
    async fn execute_with_limits(
        script: &str,
        chat: Option<Arc<dyn ChatMessageSource>>,
        limits: impl FnOnce(ExecutionLimits) -> ExecutionLimits,
    ) -> bashkit::Result<ExecResult> {
        let javascript = Arc::new(Javascript::new(Arc::default(), chat));
        let mut bash = Bash::builder()
            .builtin("js", javascript.builtin("js"))
            .limits(limits(
                ExecutionLimits::new().timeout(Duration::from_secs(5)),
            ))
            .build();
        // Modules need a file/stdin entry; `-e` parses only expressions.
        let command = format!("js - <<'JS'\n{script}\nJS");
        let result = bash.exec(&command).await;
        javascript.finish().await.unwrap();
        result
    }

    async fn run(script: &str) -> serde_json::Value {
        let chat = FakeChat::new(vec![
            FakeChat::text("user", "hello there"),
            FakeChat::text("assistant", "line one\nline two\nline three"),
        ]);
        let result = execute(script, Some(chat)).await;
        assert_eq!(result.exit_code, 0, "{}", result.stderr);
        serde_json::from_slice(result.stdout.as_bytes()).expect("structured stdout")
    }

    #[tokio::test]
    async fn get_message_returns_indexed_fields() {
        let value = run(r#"import { chat } from '@tauritavern/runtime';
console.log(JSON.stringify(chat.getMessage(1)));"#)
        .await;
        assert_eq!(value["ok"], true);
        assert_eq!(value["index"], 1);
        assert_eq!(value["role"], "assistant");
        assert_eq!(value["text"], "line one\nline two\nline three");
        assert_eq!(value["ref"], "chat:current#1:L1-L3");
        assert_eq!(value["totalBytes"], "line one\nline two\nline three".len());
        assert_eq!(value["totalMessages"], 2);
    }

    #[tokio::test]
    async fn line_range_selects_a_window() {
        let chat = FakeChat::new(vec![
            FakeChat::text("user", "hello there"),
            FakeChat::text("assistant", "line one\nline two\nline three"),
        ]);
        let result = execute(
            r#"import { chat } from '@tauritavern/runtime';
console.log(JSON.stringify(chat.getMessage(1, { startLine: 2, lineCount: 1 })));"#,
            Some(chat.clone()),
        )
        .await;
        assert_eq!(result.exit_code, 0, "{}", result.stderr);

        // The options must reach the source as a `MessageRange`, not be flattened
        // into the index or applied by the JS layer.
        assert_eq!(
            chat.request_for(0),
            (
                1,
                Some(MessageRange {
                    start_line: 2,
                    line_count: Some(1),
                })
            )
        );

        let value: serde_json::Value = serde_json::from_slice(result.stdout.as_bytes()).unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["text"], "line two");
        assert_eq!(value["startLine"], 2);
        assert_eq!(value["endLine"], 2);
        assert_eq!(value["totalLines"], 3);
    }

    #[tokio::test]
    async fn get_messages_reads_the_batch_in_one_call() {
        let chat = FakeChat::new(vec![
            FakeChat::text("user", "one"),
            FakeChat::text("assistant", "two"),
            FakeChat::text("user", "three"),
        ]);
        let result = execute(
            r#"import { chat } from '@tauritavern/runtime';
console.log(JSON.stringify(chat.getMessages([0, 1, 2])));"#,
            Some(chat.clone()),
        )
        .await;
        assert_eq!(result.exit_code, 0, "{}", result.stderr);

        // The whole request must reach the source as a single scan.
        assert_eq!(chat.calls.load(Ordering::SeqCst), 1);
        assert_eq!(chat.seen.lock().unwrap().len(), 3);

        let value: serde_json::Value = serde_json::from_slice(result.stdout.as_bytes()).unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["messages"].as_array().unwrap().len(), 3);
        assert_eq!(value["totalMessages"], 3);
        assert_eq!(value["messages"][1]["text"], "two");
    }

    #[tokio::test]
    async fn missing_index_reports_not_found_without_throwing() {
        let value = run(r#"import { chat } from '@tauritavern/runtime';
console.log(JSON.stringify(chat.getMessage(9)));"#)
        .await;
        assert_eq!(value["ok"], false);
        assert_eq!(value["reason"], "chat.message_not_found");
        assert_eq!(value["totalMessages"], 2);
    }

    #[tokio::test]
    async fn oversized_message_reports_too_large() {
        // The over-limit rule is enforced by the source; the JS surface must turn
        // the outcome into a non-throwing result object that carries its numbers.
        let chat = FakeChat::forced(ChatMessageOutcome::MessageTooLarge {
            index: 2,
            total_bytes: 50_000,
            max_bytes: 40_000,
        });
        let result = execute(
            r#"import { chat } from '@tauritavern/runtime';
console.log(JSON.stringify(chat.getMessage(2)));"#,
            Some(chat),
        )
        .await;
        assert_eq!(result.exit_code, 0, "{}", result.stderr);

        let value: serde_json::Value = serde_json::from_slice(result.stdout.as_bytes()).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["reason"], "chat.message_too_large");
        assert_eq!(value["maxBytes"], 40_000);
        assert_eq!(value["totalBytes"], 50_000);
    }

    #[tokio::test]
    async fn invalid_range_reports_invalid_message_range() {
        // As with the over-limit case, the range verdict is the source's; here the
        // JS surface only has to shape it.
        let chat = FakeChat::forced(ChatMessageOutcome::InvalidRange {
            index: 0,
            message: "start_line 99 is beyond total lines 1".into(),
        });
        let result = execute(
            r#"import { chat } from '@tauritavern/runtime';
console.log(JSON.stringify(chat.getMessage(0, { startLine: 99 })));"#,
            Some(chat),
        )
        .await;
        assert_eq!(result.exit_code, 0, "{}", result.stderr);

        let value: serde_json::Value = serde_json::from_slice(result.stdout.as_bytes()).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["reason"], "chat.invalid_message_range");
        assert_eq!(value["index"], 0);
    }

    #[tokio::test]
    async fn unsupported_chat_reports_capability_absent() {
        let result = execute(
            r#"import { chat } from '@tauritavern/runtime';
console.log(JSON.stringify(chat.getMessage(0)));"#,
            Some(Arc::new(UnsupportedChat)),
        )
        .await;
        assert_eq!(result.exit_code, 0, "{}", result.stderr);
        let value: serde_json::Value = serde_json::from_slice(result.stdout.as_bytes()).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["reason"], "chat.unsupported");
    }

    #[tokio::test]
    async fn absent_chat_capability_reports_unsupported() {
        let result = execute(
            r#"import { chat } from '@tauritavern/runtime';
console.log(JSON.stringify(chat.getMessages([0])));"#,
            None,
        )
        .await;
        assert_eq!(result.exit_code, 0, "{}", result.stderr);
        let value: serde_json::Value = serde_json::from_slice(result.stdout.as_bytes()).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["reason"], "chat.unsupported");
    }

    #[tokio::test]
    async fn a_source_failure_throws_instead_of_reporting_a_reason() {
        // Only the documented outcomes are recoverable verdicts. A read that fails
        // outright has to fail loudly, matching `chat.read_messages`.
        let result = execute(
            r#"import { chat } from '@tauritavern/runtime';
console.log(JSON.stringify(chat.getMessage(0)));"#,
            Some(Arc::new(FailingChat)),
        )
        .await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty(), "{}", result.stdout);
        assert!(
            result
                .stderr
                .text_lossy()
                .contains("macro expansion exceeds"),
            "{}",
            result.stderr
        );
    }

    #[tokio::test]
    async fn malformed_arguments_throw() {
        for script in [
            "chat.getMessage(-1)",
            "chat.getMessage('0')",
            "chat.getMessages(0)",
            "chat.getMessages([0, 'x'])",
            "chat.getMessages([])",
            "chat.getMessage(0, { lineCount: 2 })",
            "chat.getMessage(0, { startLine: 0 })",
        ] {
            let result = execute(
                &format!("import {{ chat }} from '@tauritavern/runtime';\n{script};"),
                None,
            )
            .await;
            assert_eq!(result.exit_code, 1, "{script}");
            assert!(result.stdout.is_empty(), "{script}: {}", result.stdout);
        }
    }

    #[tokio::test]
    async fn chat_text_is_charged_against_the_shell_input_budget() {
        // Chat history and workspace files share one budget, so every read charges
        // the text it hands the script; a read that does not fit must stop the shell
        // rather than grow the QuickJS heap past it.
        let body = "x".repeat(60_000);
        let source = || FakeChat::new(vec![FakeChat::text("user", body.clone())]);
        let once = r#"import { chat } from '@tauritavern/runtime';
console.log(`read ${chat.getMessage(0).ok}`);"#;
        let twice = r#"import { chat } from '@tauritavern/runtime';
chat.getMessage(0);
console.log(`read ${chat.getMessage(0).ok}`);"#;

        // One read of 60 KB fits a 100 KB budget; the 501 bytes bashkit charges for
        // the script itself leave the threshold clear either way.
        let single = execute_with_limits(once, Some(source()), |limits| {
            limits.max_aggregate_input_bytes(100_000)
        })
        .await
        .expect("one read fits the budget");
        assert!(single.stdout.text_lossy().contains("read true"));

        let over = execute_with_limits(twice, Some(source()), |limits| {
            limits.max_aggregate_input_bytes(100_000)
        })
        .await
        .expect_err("two reads must not fit the budget");
        assert!(over.to_string().contains("aggregate input bytes"), "{over}");

        // The same script under a budget with room for both reads succeeds, so the
        // failure above is the accounting and not an unconditional error.
        let roomy = execute_with_limits(twice, Some(source()), |limits| {
            limits.max_aggregate_input_bytes(1_000_000)
        })
        .await
        .expect("two reads fit a roomy budget");
        assert!(roomy.stdout.text_lossy().contains("read true"));
    }

    #[tokio::test]
    async fn a_call_over_the_budget_reports_its_measured_size() {
        // Like the per-message limit, the per-call budget is the source's verdict;
        // the JS surface returns it without throwing so the script can narrow the
        // request instead of losing the call.
        let chat = FakeChat::forced(ChatMessageOutcome::CallTooLarge {
            index: 3,
            used_bytes: 9_000_000,
            max_bytes: 8_388_608,
        });
        let result = execute(
            r#"import { chat } from '@tauritavern/runtime';
console.log(JSON.stringify(chat.getMessages([1, 2, 3])));"#,
            Some(chat),
        )
        .await;
        assert_eq!(result.exit_code, 0, "{}", result.stderr);

        let value: serde_json::Value = serde_json::from_slice(result.stdout.as_bytes()).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["reason"], "chat.call_too_large");
        assert_eq!(value["index"], 3);
        assert_eq!(value["usedBytes"], 9_000_000);
        assert_eq!(value["maxBytes"], 8_388_608);
    }

    #[tokio::test]
    async fn a_batch_reaches_the_source_without_a_count_ceiling() {
        // The per-call byte budget bounds a read now, so the length of the index
        // list is not what the surface refuses.
        let chat = FakeChat::new(vec![FakeChat::text("user", "one")]);
        let result = execute(
            r#"import { chat } from '@tauritavern/runtime';
const indices = [];
for (let i = 0; i < 600; i += 1) indices.push(0);
console.log(JSON.stringify(chat.getMessages(indices).messages.length));"#,
            Some(chat.clone()),
        )
        .await;
        assert_eq!(result.exit_code, 0, "{}", result.stderr);
        assert_eq!(chat.seen.lock().unwrap().len(), 600);
        assert_eq!(result.stdout.text_lossy().trim(), "600");
    }
}
