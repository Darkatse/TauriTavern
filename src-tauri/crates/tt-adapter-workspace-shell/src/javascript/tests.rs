use std::sync::Arc;
use std::time::Duration;

use bashkit::{Bash, ExecResult, ExecutionLimits};

use super::Javascript;
use crate::engine::MAX_OUTPUT_BYTES;

async fn execute(command: &str) -> bashkit::Result<ExecResult> {
    let javascript = Arc::new(Javascript::new(Arc::default()));
    let mut bash = Bash::builder()
        .builtin("js", javascript.builtin("js"))
        .limits(ExecutionLimits::new().timeout(Duration::from_secs(1)))
        .build();
    let result = bash.exec(command).await;
    javascript.finish().await.unwrap();
    result
}

#[tokio::test]
async fn awaited_export_keeps_json_separate_from_diagnostics() {
    let result = execute(
        r#"
js --call report --args-json '{"name":"draft"}' - <<'JS'
const version = await Promise.resolve(2);
export async function report({ name }) {
    console.log('processing', name);
    return { name, version, ready: await Promise.resolve(true) };
}
JS
"#,
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(result.stdout.as_bytes()).unwrap(),
        serde_json::json!({ "name": "draft", "version": 2, "ready": true }),
    );
    assert!(result.stderr.text_lossy().contains("processing draft"));
}

#[tokio::test]
async fn execution_limits_report_failure_without_partial_json() {
    let timeout = execute("js -e 'while (true) {}'").await.unwrap_err();
    assert!(
        matches!(timeout, bashkit::Error::ResourceLimit(_)),
        "{timeout}"
    );
    let oversized_result = format!(
        "js --call default -e 'export default () => ({{ text: \"x\".repeat({}) }})'",
        MAX_OUTPUT_BYTES + 1,
    );
    for (command, reason) in [
        ("js -e 'await new Promise(() => {})'", "Promise"),
        (oversized_result.as_str(), "output"),
    ] {
        let result = execute(command).await.unwrap();
        assert_ne!(result.exit_code, 0, "{command}");
        assert!(result.stdout.is_empty(), "{command}: {}", result.stdout);
        assert!(
            result.stderr.text_lossy().contains(reason),
            "{command}: {}",
            result.stderr,
        );
    }
}
