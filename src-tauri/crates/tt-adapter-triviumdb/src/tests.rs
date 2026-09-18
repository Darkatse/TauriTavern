use std::future::poll_fn;
use std::task::Poll;

use serde_json::{Value, json};
use tt_contracts::database::DatabaseRequest;
use tt_domain::errors::DomainError;
use tt_ports::database::DatabaseBackend;

use super::TriviumDatabaseBackend;

async fn request(backend: &TriviumDatabaseBackend, value: Value) -> Result<Value, DomainError> {
    let request = serde_json::from_value(value).unwrap();
    backend
        .execute(request)
        .await
        .map(|result| serde_json::to_value(result).unwrap())
}

async fn open(backend: &TriviumDatabaseBackend, options: Value) -> Result<Value, DomainError> {
    request(
        backend,
        json!({"type": "open", "namespace": "memory", "options": options}),
    )
    .await
}

async fn operation(backend: &TriviumDatabaseBackend, value: Value) -> Result<Value, DomainError> {
    request(
        backend,
        json!({"type": "execute", "namespace": "memory", "operation": value}),
    )
    .await
}

async fn close(backend: &TriviumDatabaseBackend) -> Result<Value, DomainError> {
    request(backend, json!({"type": "close", "namespace": "memory"})).await
}

#[tokio::test]
async fn failed_batches_do_not_commit_and_open_options_cannot_silently_change() {
    let root = tempfile::tempdir().unwrap();
    let backend = TriviumDatabaseBackend::new(root.path().into());
    open(
        &backend,
        json!({"dim": 2, "syncMode": "full", "autoBuildQuiver": false}),
    )
    .await
    .unwrap();
    let reused = open(&backend, json!({})).await.unwrap();
    assert_eq!(reused["options"]["syncMode"], "full");
    assert!(matches!(
        open(&backend, json!({"dim": 3})).await,
        Err(DomainError::Conflict(_))
    ));
    assert!(matches!(
        open(&backend, json!({"syncMode": "off"})).await,
        Err(DomainError::Conflict(_))
    ));
    assert!(matches!(
        operation(
            &backend,
            json!({
                "type": "batchInsert", "vectors": [[1, 0], [1]], "payloads": [{}, {}],
            })
        )
        .await,
        Err(DomainError::InvalidData(_))
    ));
    assert_eq!(
        operation(&backend, json!({"type": "stats"})).await.unwrap()["nodeCount"],
        0
    );
    let ids = operation(&backend, json!({
        "type": "batchInsert", "vectors": [[1, 0], [0, 1]], "payloads": [{"name": "a"}, {"name": "b"}],
    })).await.unwrap();
    assert_eq!(ids.as_array().unwrap().len(), 2);
    close(&backend).await.unwrap();
    assert!(matches!(
        operation(&backend, json!({"type": "stats"})).await,
        Err(DomainError::NotFound(_))
    ));
    assert!(matches!(
        open(&backend, json!({"dim": 3})).await,
        Err(DomainError::Conflict(_))
    ));
    assert_eq!(open(&backend, json!({})).await.unwrap()["dim"], 2);
    assert_eq!(
        operation(&backend, json!({"type": "stats"})).await.unwrap()["nodeCount"],
        2
    );
    close(&backend).await.unwrap();
}

#[tokio::test]
async fn persisted_node_ids_round_trip_through_graph_search_and_tql() {
    let root = tempfile::tempdir().unwrap();
    let backend = TriviumDatabaseBackend::new(root.path().into());
    open(&backend, json!({"dim": 2, "autoBuildQuiver": false}))
        .await
        .unwrap();
    let source_id = operation(
        &backend,
        json!({"type": "insert", "vector": [0, 1], "payload": {"name": "source"}}),
    )
    .await
    .unwrap();
    let target_id = 42;
    operation(
        &backend,
        json!({"type": "upsert", "id": target_id, "vector": [1, 0], "payload": {"name": "target"}}),
    )
    .await
    .unwrap();
    operation(
        &backend,
        json!({"type": "link", "src": source_id, "dst": target_id, "label": "related", "weight": 1}),
    )
    .await
    .unwrap();
    close(&backend).await.unwrap();
    open(&backend, json!({})).await.unwrap();
    let node = operation(&backend, json!({"type": "get", "id": source_id}))
        .await
        .unwrap();
    assert_eq!(node["edges"][0]["targetId"], target_id);
    let hits = operation(
        &backend,
        json!({"type": "search", "vector": [1, 0], "config": {"expandDepth": 0}}),
    )
    .await
    .unwrap();
    assert_eq!(hits[0]["id"], target_id);
    let result = operation(
        &backend,
        json!({
            "type": "query", "query": "MATCH (n) WHERE n.id == $id RETURN n, n.id AS id",
            "params": {"id": hits[0]["id"]},
        }),
    )
    .await
    .unwrap();
    assert_eq!(result["rows"][0]["n"]["value"]["id"], target_id);
    assert_eq!(
        result["rows"][0]["id"],
        json!({"type": "integer", "value": target_id})
    );
    let by_id = operation(
        &backend,
        json!({
            "type": "query", "query": "MATCH (n) WHERE n.id == $id RETURN COLLECT(n.id) AS ids",
            "params": {"id": hits[0]["id"]},
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        by_id["rows"],
        json!([{"ids": {"type": "list", "value": [target_id]}}])
    );
    close(&backend).await.unwrap();
}

#[tokio::test]
async fn reopening_without_text_queries_preserves_explicit_index_content() {
    let root = tempfile::tempdir().unwrap();
    let backend = TriviumDatabaseBackend::new(root.path().into());
    open(&backend, json!({"dim": 2, "loadTextIndex": false}))
        .await
        .unwrap();
    let id = operation(
        &backend,
        json!({"type": "insert", "vector": [1, 0], "payload": {"kind": "memory"}}),
    )
    .await
    .unwrap();
    operation(
        &backend,
        json!({"type": "indexText", "id": id, "text": "secretbell opens the gate"}),
    )
    .await
    .unwrap();
    operation(&backend, json!({"type": "buildTextIndex"}))
        .await
        .unwrap();
    close(&backend).await.unwrap();
    open(&backend, json!({"loadTextIndex": false}))
        .await
        .unwrap();
    // Closing without a text query must not overwrite an unloaded index.
    close(&backend).await.unwrap();
    open(&backend, json!({})).await.unwrap();
    let hits = operation(
        &backend,
        json!({"type": "search", "queryText": "secretbell", "config": {"expandDepth": 0}}),
    )
    .await
    .unwrap();
    assert_eq!(hits[0]["id"], id);
    close(&backend).await.unwrap();
}

#[tokio::test]
async fn tql_binds_values_once_and_uses_native_statement_classification() {
    let root = tempfile::tempdir().unwrap();
    let backend = TriviumDatabaseBackend::new(root.path().into());
    open(&backend, json!({"dim": 2})).await.unwrap();
    let text = "quote ' slash \\ newline\n carriage\r nul\0 $id2 中文";
    let payload =
        json!({"text": text, "items": [true, null, "λ"], "id": 1, "id2": 2, "literal": "$id"});
    let created = operation(
        &backend,
        json!({
            "type": "query", "query": "create ($payload)", "params": {"payload": payload},
        }),
    )
    .await
    .unwrap();
    assert_eq!(created["type"], "mutation");
    let id = created["createdIds"][0].clone();
    assert_eq!(
        operation(&backend, json!({"type": "get", "id": id}))
            .await
            .unwrap()["payload"],
        payload
    );
    let found = operation(&backend, json!({"type": "query", "query": "FIND {id: $id, id2: $id2, literal: '$id'} RETURN * -- $ignored", "params": {"id": 1, "id2": 2}})).await.unwrap();
    assert_eq!(found["rows"].as_array().unwrap().len(), 1);
    let found = operation(&backend, json!({"type": "query", "query": "MATCH (n) WHERE n.text == $text RETURN n", "params": {"text": text}})).await.unwrap();
    assert_eq!(found["rows"][0]["n"]["value"]["payload"], payload);
    operation(
        &backend,
        json!({"type": "updatePayload", "id": id, "payload": {"text": "DELETE", "age": 2}}),
    )
    .await
    .unwrap();
    let found = operation(
        &backend,
        json!({"type": "query", "query": "MATCH (n) WHERE n.text == 'DELETE' RETURN n"}),
    )
    .await
    .unwrap();
    assert_eq!(found["type"], "query");
    let found = operation(&backend, json!({"type": "query", "query": "FIND {age: {$gte: $gte}} RETURN *", "params": {"gte": 1}})).await.unwrap();
    assert_eq!(found["rows"].as_array().unwrap().len(), 1);
    operation(
        &backend,
        json!({"type": "updateVector", "id": id, "vector": [1, 0]}),
    )
    .await
    .unwrap();
    let found = operation(&backend, json!({"type": "query", "query": "SEARCH VECTOR $vec TOP 1 RETURN *", "params": {"vec": [1, 0]}})).await.unwrap();
    assert_eq!(found["rows"].as_array().unwrap().len(), 1);
    assert!(matches!(
        operation(
            &backend,
            json!({"type": "query", "query": "CREATE ($missing)"})
        )
        .await,
        Err(DomainError::InvalidData(_))
    ));
    close(&backend).await.unwrap();
}

#[tokio::test]
async fn failed_close_keeps_the_instance_available_for_retry() {
    let root = tempfile::tempdir().unwrap();
    let backend = TriviumDatabaseBackend::new(root.path().into());
    open(&backend, json!({"dim": 2})).await.unwrap();
    let id = operation(&backend, json!({"type": "insert", "vector": [1, 0]}))
        .await
        .unwrap();
    // Refuse creation of the engine's checkpoint temp file without platform permissions.
    let blocker = root.path().join("db-memory/database.tdb.tmp");
    std::fs::create_dir(&blocker).unwrap();
    assert!(close(&backend).await.is_err());
    assert_eq!(
        request(&backend, json!({"type": "listNamespaces"}))
            .await
            .unwrap(),
        json!(["memory"])
    );
    assert_eq!(
        operation(&backend, json!({"type": "get", "id": id}))
            .await
            .unwrap()["id"],
        id
    );
    std::fs::remove_dir(blocker).unwrap();
    close(&backend).await.unwrap();
    open(&backend, json!({"dim": 2})).await.unwrap();
    assert_eq!(
        operation(&backend, json!({"type": "stats"})).await.unwrap()["nodeCount"],
        1
    );
    close(&backend).await.unwrap();
}

#[tokio::test]
async fn queued_close_then_reopen_share_one_instance_owner() {
    let root = tempfile::tempdir().unwrap();
    let backend = TriviumDatabaseBackend::new(root.path().into());
    open(&backend, json!({"dim": 2})).await.unwrap();
    let id = operation(&backend, json!({"type": "insert", "vector": [1, 0]}))
        .await
        .unwrap();
    let slot = backend.slot("memory").unwrap();
    let guard = slot.lock().await;
    let mut closing = backend.execute(DatabaseRequest::Close {
        namespace: "memory".into(),
    });
    let mut reopening = backend.execute(
        serde_json::from_value(
            json!({"type": "open", "namespace": "memory", "options": {"dim": 2}}),
        )
        .unwrap(),
    );
    assert!(
        poll_fn(|cx| Poll::Ready(closing.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(
        poll_fn(|cx| Poll::Ready(reopening.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    drop(guard);
    let (closed, opened) = tokio::join!(closing, reopening);
    closed.unwrap();
    opened.unwrap();
    assert_eq!(
        operation(&backend, json!({"type": "get", "id": id}))
            .await
            .unwrap()["id"],
        id
    );
    close(&backend).await.unwrap();
}
