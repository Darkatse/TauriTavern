use std::path::PathBuf;

use serde_json::{Value, json};
use tokio::fs as tokio_fs;
use uuid::Uuid;

use super::*;
use crate::domain::models::skill::{
    SkillImportConflictKind, SkillImportInput, SkillInlineFile, SkillInstallAction,
    SkillInstallConflictStrategy, SkillInstallRequest,
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

    let read = repository
        .read_skill_file("test-skill", "references/a.md", None)
        .await
        .expect("read skill file");
    assert_eq!(read.content, "hello");
    assert_eq!(read.resource_ref, "skills/test-skill/references/a.md");

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
        .read_skill_file("test-skill", "references/a.md", None)
        .await
        .expect_err("symlink escape should fail");
    assert!(error.to_string().contains("escapes installed directory"));

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}
