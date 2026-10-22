use std::sync::Arc;
use std::time::Duration;

use bashkit::{Bash, ExecResult, ExecutionLimits};

use super::Javascript;
use crate::engine::MAX_OUTPUT_BYTES;

async fn execute(command: &str) -> bashkit::Result<ExecResult> {
    let javascript = Arc::new(Javascript::new(Arc::default()));
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
