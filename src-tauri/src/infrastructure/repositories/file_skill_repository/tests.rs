use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde_json::{Value, json};
use tokio::fs as tokio_fs;
use uuid::Uuid;

use super::*;
use crate::domain::models::skill::{
    SkillFileKind, SkillImportConflictKind, SkillImportInput, SkillInlineFile, SkillInstallAction,
    SkillInstallConflictStrategy, SkillInstallRequest, SkillReadRequest, SkillSearchRequest,
};
use crate::domain::repositories::skill_repository::SkillRepository;

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tauritavern-skill-{label}-{}",
        Uuid::new_v4().simple()
    ))
}

fn inline_skill(name: &str, extra_files: Vec<(&str, &str)>) -> SkillImportInput {
    inline_skill_with_source(name, extra_files, json!({"kind": "test"}))
}

fn inline_skill_with_source(
    name: &str,
    extra_files: Vec<(&str, &str)>,
    source: Value,
) -> SkillImportInput {
    let mut files = vec![SkillInlineFile {
        path: "SKILL.md".to_string(),
        encoding: "utf8".to_string(),
        content: format!(
            "---\nname: {name}\ndescription: Use for testing Skill imports.\nmetadata:\n  tags:\n    - tests\n---\n\n# Test\n"
        ),
        media_type: None,
        size_bytes: None,
        sha256: None,
    }];
    files.extend(
        extra_files
            .into_iter()
            .map(|(path, content)| SkillInlineFile {
                path: path.to_string(),
                encoding: "utf8".to_string(),
                content: content.to_string(),
                media_type: None,
                size_bytes: None,
                sha256: None,
            }),
    );
    SkillImportInput::InlineFiles { files, source }
}

#[tokio::test]
async fn installs_inline_skill_and_reads_file() {
    let root = temp_root("install");
    let repository = FileSkillRepository::new(root.clone());
    let result = repository
        .install_import(SkillInstallRequest {
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    assert_eq!(result.action, SkillInstallAction::Installed);
    let listed = repository.list_skills().await.expect("list skills");
    assert_eq!(listed[0].name, "test-skill");
    assert_eq!(listed[0].tags, vec!["tests"]);
    let files = repository
        .list_skill_files("test-skill")
        .await
        .expect("list skill files");
    assert_eq!(
        files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["SKILL.md", "references/a.md"]
    );
    assert_eq!(files[1].kind, SkillFileKind::Text);

    let read = repository
        .read_skill_file(SkillReadRequest {
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: None,
        })
        .await
        .expect("read skill file");
    assert_eq!(read.content, "hello");
    assert_eq!(read.resource_ref, "skills/test-skill/references/a.md");

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn reads_skill_file_ranges() {
    let root = temp_root("read-ranges");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            input: inline_skill(
                "test-skill",
                vec![("references/a.md", "alpha\nblue lantern\nomega")],
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let line = repository
        .read_skill_file(SkillReadRequest {
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: Some(2),
            line_count: Some(1),
            start_char: None,
            max_chars: Some(80),
        })
        .await
        .expect("read line range");
    assert_eq!(line.content, "blue lantern");
    assert_eq!(line.start_line, 2);
    assert_eq!(line.end_line, 2);
    assert!(line.truncated);

    let chars = repository
        .read_skill_file(SkillReadRequest {
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: Some(6),
            max_chars: Some(4),
        })
        .await
        .expect("read char range");
    assert_eq!(chars.content, "blue");
    assert_eq!(chars.start_char, 6);
    assert_eq!(chars.end_char, 10);

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn searches_installed_skill_text_files() {
    let root = temp_root("search");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            input: inline_skill(
                "test-skill",
                vec![("references/a.md", "alpha\nblue lantern\nomega")],
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let search = repository
        .search_skill_files(SkillSearchRequest {
            name: "test-skill".to_string(),
            query: "blue lantern".to_string(),
            path: Some("references".to_string()),
            limit: 5,
            context_lines: 0,
        })
        .await
        .expect("search skill");
    assert_eq!(search.searched_files, 1);
    assert_eq!(search.hits[0].path, "references/a.md");
    assert_eq!(search.hits[0].start_line, 2);
    assert!(search.hits[0].snippet.contains("blue lantern"));

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn requires_explicit_replace_on_conflict() {
    let root = temp_root("conflict");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            input: inline_skill("test-skill", vec![("references/a.md", "one")]),
            conflict_strategy: None,
        })
        .await
        .expect("install initial");

    let error = repository
        .install_import(SkillInstallRequest {
            input: inline_skill("test-skill", vec![("references/a.md", "two")]),
            conflict_strategy: None,
        })
        .await
        .expect_err("conflict should fail");
    assert!(error.to_string().contains("conflict_strategy is required"));

    let replaced = repository
        .install_import(SkillInstallRequest {
            input: inline_skill("test-skill", vec![("references/a.md", "two")]),
            conflict_strategy: Some(SkillInstallConflictStrategy::Replace),
        })
        .await
        .expect("replace skill");
    assert_eq!(replaced.action, SkillInstallAction::Replaced);

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn deletes_skill_when_last_linked_source_is_deleted() {
    let root = temp_root("delete-source-last");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            input: inline_skill_with_source(
                "test-skill",
                vec![],
                json!({"kind":"preset","id":"preset:openai:One","label":"One"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let deleted = repository
        .delete_skills_for_source("preset", "preset:openai:One")
        .await
        .expect("delete linked skills");
    assert_eq!(deleted, vec!["test-skill"]);
    assert!(repository.list_skills().await.expect("list").is_empty());
    assert!(!root.join("installed").join("test-skill").exists());

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn deletes_selected_skill() {
    let root = temp_root("delete-selected");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    repository
        .delete_skill("test-skill")
        .await
        .expect("delete selected skill");

    assert!(repository.list_skills().await.expect("list").is_empty());
    assert!(!root.join("installed").join("test-skill").exists());
    let error = repository
        .delete_skill("test-skill")
        .await
        .expect_err("missing skill should fail");
    assert!(error.to_string().contains("Skill not found: test-skill"));

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn keeps_skill_until_all_linked_sources_are_deleted() {
    let root = temp_root("delete-source-shared");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            input: inline_skill_with_source(
                "test-skill",
                vec![],
                json!({"kind":"preset","id":"preset:openai:One","label":"One"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install first source");
    repository
        .install_import(SkillInstallRequest {
            input: inline_skill_with_source(
                "test-skill",
                vec![],
                json!({"kind":"character","id":"character:Aurelia","label":"Aurelia"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("link same skill to second source");

    let deleted = repository
        .delete_skills_for_source("preset", "preset:openai:One")
        .await
        .expect("delete first source");
    assert!(deleted.is_empty());
    let listed = repository.list_skills().await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].source_refs.len(), 1);
    assert_eq!(listed[0].source_refs[0].id, "character:Aurelia");

    let deleted = repository
        .delete_skills_for_source("character", "character:Aurelia")
        .await
        .expect("delete second source");
    assert_eq!(deleted, vec!["test-skill"]);
    assert!(repository.list_skills().await.expect("list").is_empty());

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn replacing_skill_drops_previous_source_links() {
    let root = temp_root("replace-source-links");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            input: inline_skill_with_source(
                "test-skill",
                vec![("references/a.md", "one")],
                json!({"kind":"preset","id":"preset:openai:One","label":"One"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install first source");
    repository
        .install_import(SkillInstallRequest {
            input: inline_skill_with_source(
                "test-skill",
                vec![("references/a.md", "two")],
                json!({"kind":"character","id":"character:Aurelia","label":"Aurelia"}),
            ),
            conflict_strategy: Some(SkillInstallConflictStrategy::Replace),
        })
        .await
        .expect("replace skill");

    let deleted = repository
        .delete_skills_for_source("preset", "preset:openai:One")
        .await
        .expect("delete old source");
    assert!(deleted.is_empty());
    let listed = repository.list_skills().await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].source_refs.len(), 1);
    assert_eq!(listed[0].source_refs[0].id, "character:Aurelia");

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn rejects_invalid_sidecar_schema() {
    let root = temp_root("sidecar");
    let repository = FileSkillRepository::new(root.clone());
    let error = repository
        .preview_import(inline_skill(
            "test-skill",
            vec![(
                "agents/tauritavern.json",
                r#"{"version":1,"unexpected":true}"#,
            )],
        ))
        .await
        .expect_err("invalid sidecar should fail");
    assert!(
        error
            .to_string()
            .contains("Invalid agents/tauritavern.json")
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn exported_skill_archive_can_be_reimported() {
    let root = temp_root("export");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let exported = repository
        .export_skill("test-skill")
        .await
        .expect("export skill");
    let archive_path = root.join("test-skill.ttskill");
    tokio_fs::write(&archive_path, exported.bytes)
        .await
        .expect("write archive");

    let second_root = temp_root("reimport");
    let second_repository = FileSkillRepository::new(second_root.clone());
    let preview = second_repository
        .preview_import(SkillImportInput::ArchiveFile {
            path: archive_path.to_string_lossy().to_string(),
            source: json!({"kind": "test"}),
        })
        .await
        .expect("preview exported archive");
    assert_eq!(preview.skill.name, "test-skill");
    assert_eq!(preview.conflict.kind, SkillImportConflictKind::New);

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
    tokio_fs::remove_dir_all(second_root)
        .await
        .expect("cleanup");
}

#[tokio::test]
async fn exported_skill_archive_base64_can_be_reimported() {
    let root = temp_root("export-base64");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let exported = repository
        .export_skill("test-skill")
        .await
        .expect("export skill");
    let second_root = temp_root("reimport-base64");
    let second_repository = FileSkillRepository::new(second_root.clone());
    let preview = second_repository
        .preview_import(SkillImportInput::ArchiveBase64 {
            file_name: exported.file_name,
            content_base64: BASE64_STANDARD.encode(exported.bytes),
            sha256: Some(exported.sha256),
            source: json!({"kind": "test"}),
        })
        .await
        .expect("preview exported archive");
    assert_eq!(preview.skill.name, "test-skill");
    assert_eq!(preview.conflict.kind, SkillImportConflictKind::New);

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
    tokio_fs::remove_dir_all(second_root)
        .await
        .expect("cleanup");
}

#[tokio::test]
async fn exported_skill_roundtrip_preserves_hash() {
    let root = temp_root("export-same");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let installed_hash = repository.list_skills().await.expect("list skills")[0]
        .installed_hash
        .clone();
    let exported = repository
        .export_skill("test-skill")
        .await
        .expect("export skill");
    let archive_path = root.join("test-skill.ttskill");
    tokio_fs::write(&archive_path, exported.bytes)
        .await
        .expect("write archive");

    let preview = repository
        .preview_import(SkillImportInput::ArchiveFile {
            path: archive_path.to_string_lossy().to_string(),
            source: json!({"kind": "test"}),
        })
        .await
        .expect("preview exported archive");

    assert_eq!(preview.conflict.kind, SkillImportConflictKind::Same);
    assert_eq!(preview.skill.installed_hash, installed_hash);

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[cfg(unix)]
#[tokio::test]
async fn read_rejects_symlink_escape_inside_installed_skill() {
    use std::os::unix::fs::symlink;

    let root = temp_root("symlink-escape");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let skill_root = root.join("installed").join("test-skill");
    let outside = root.join("outside");
    tokio_fs::create_dir_all(&outside)
        .await
        .expect("create outside dir");
    tokio_fs::write(outside.join("a.md"), "outside")
        .await
        .expect("write outside file");
    tokio_fs::remove_dir_all(skill_root.join("references"))
        .await
        .expect("remove references dir");
    symlink(&outside, skill_root.join("references")).expect("create symlink");

    let error = repository
        .read_skill_file(SkillReadRequest {
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: None,
        })
        .await
        .expect_err("symlink escape should fail");
    assert!(error.to_string().contains("escapes installed directory"));

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}
