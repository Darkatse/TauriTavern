use super::*;
use tt_application::services::world_info_service::WorldInfoService;

#[tokio::test]
async fn world_info_document_replacement_preserves_text_and_rejects_invalid_updates() {
    let root = temp_root("world-info-document");
    let user = root.join("default-user");
    let service = WorldInfoService::new(Arc::new(FileWorldInfoRepository::new(
        user.join("worlds"),
        Arc::default(),
        4 * 1024 * 1024,
    )));
    let json = r#"{ "z":9007199254740993,"entries":{},"originalData":{"b":"你好 👋","a":1,"opaque":"\ud800"} }"#;
    let request = format!(
        r#"{{"name":false,"data":null,"extra":{{"data":"unrelated }}"}},"data": {json}, "name":" 世界 "}}"#
    );
    let session = service.begin_commit().await.unwrap();
    let mut offset = 0;
    for bytes in request.as_bytes().chunks(7) {
        offset = service
            .append_commit(&session.session_id, offset, bytes)
            .await
            .unwrap();
    }
    service
        .finish_commit(&session.session_id, offset)
        .await
        .unwrap();
    assert_eq!(
        service.get_world_info(" 世界 ").await.unwrap().bytes,
        json.as_bytes()
    );

    // A complete body must not be published if the enclosing request is malformed.
    let invalid = r#"{"name":" 世界 ","data":{"entries":{}},}"#.as_bytes();
    let session = service.begin_commit().await.unwrap();
    let size = service
        .append_commit(&session.session_id, 0, invalid)
        .await
        .unwrap();
    assert!(
        service
            .finish_commit(&session.session_id, size)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(user.join("worlds/ 世界 .json")).await.unwrap(),
        json.as_bytes()
    );
    assert_eq!(
        std::fs::read_dir(user.join(".staging/world-info"))
            .unwrap()
            .count(),
        0
    );
    fs::remove_dir_all(root).await.unwrap();
}
