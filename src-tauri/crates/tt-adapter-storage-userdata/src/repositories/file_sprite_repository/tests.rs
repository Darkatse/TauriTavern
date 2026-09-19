use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use tt_domain::errors::DomainError;
use tt_ports::repositories::sprite_repository::{SpriteName, SpriteRepository, SpriteSet};
use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::FileSpriteRepository;

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tauritavern-sprites-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).expect("create temp directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let file = File::create(path).expect("create zip");
    let mut writer = ZipWriter::new(file);
    for (name, bytes) in entries {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("start zip entry");
        writer.write_all(bytes).expect("write zip entry");
    }
    writer.finish().expect("finish zip");
}

#[tokio::test]
async fn upload_list_replace_and_delete_follow_sprite_stem_semantics() {
    let temp = TempDirGuard::new("lifecycle");
    let characters = temp.path().join("characters");
    let repository = FileSpriteRepository::new(characters.clone());
    let set = SpriteSet::parse("Alice/formal").expect("sprite set");
    let name = SpriteName::parse("joy").expect("sprite name");

    assert!(
        repository
            .list(&set)
            .await
            .expect("missing list")
            .is_empty()
    );
    let missing = SpriteSet::parse("Missing").expect("missing set");
    assert!(matches!(
        repository.delete(&missing, &name).await,
        Err(DomainError::NotFound(_))
    ));

    let first = temp.path().join("first.png");
    tokio::fs::write(&first, b"png").await.expect("write png");
    repository
        .upload(&set, &name, "first.png", &first)
        .await
        .expect("upload png");

    let second = temp.path().join("second.webp");
    tokio::fs::write(&second, b"webp")
        .await
        .expect("write webp");
    repository
        .upload(&set, &name, "second.webp", &second)
        .await
        .expect("replace with webp");
    tokio::fs::write(characters.join("Alice/formal/readme.txt"), b"ignored")
        .await
        .expect("write non-image");

    let sprites = repository.list(&set).await.expect("list sprites");
    assert_eq!(
        sprites
            .iter()
            .map(|sprite| sprite.file_name.as_str())
            .collect::<Vec<_>>(),
        vec!["joy.webp"]
    );
    assert!(!characters.join("Alice/formal/joy.png").exists());
    assert_eq!(
        tokio::fs::read(characters.join("Alice/formal/joy.webp"))
            .await
            .expect("read replacement"),
        b"webp"
    );

    repository.delete(&set, &name).await.expect("delete sprite");
    assert!(
        repository
            .list(&set)
            .await
            .expect("list after delete")
            .is_empty()
    );
}

#[tokio::test]
async fn sprite_pack_flattens_images_and_ignores_unselected_limits() {
    let temp = TempDirGuard::new("pack");
    let characters = temp.path().join("characters");
    let set_dir = characters.join("Alice");
    std::fs::create_dir_all(&set_dir).expect("create set");
    std::fs::write(set_dir.join("joy.gif"), b"old").expect("write old sprite");

    let archive = temp.path().join("sprites.zip");
    let ignored = vec![0; 64 * 1024];
    write_zip(
        &archive,
        &[
            ("joy.png", b"new"),
            ("wrapper/sad.webp", b"sad"),
            ("readme.txt", &ignored),
            ("__MACOSX/._joy.png", b"ignored"),
        ],
    );

    let repository = FileSpriteRepository::new(characters.clone());
    let set = SpriteSet::parse("Alice").expect("sprite set");
    assert_eq!(
        repository
            .upload_pack(&set, &archive)
            .await
            .expect("import pack"),
        2
    );
    assert_eq!(std::fs::read(set_dir.join("joy.png")).unwrap(), b"new");
    assert_eq!(std::fs::read(set_dir.join("sad.webp")).unwrap(), b"sad");
    assert!(!set_dir.join("joy.gif").exists());
}

#[tokio::test]
async fn sprite_pack_rejects_unsafe_images_before_writing() {
    let temp = TempDirGuard::new("invalid-pack");
    let characters = temp.path().join("characters");
    let repository = FileSpriteRepository::new(characters.clone());
    let set = SpriteSet::parse("Alice").expect("sprite set");

    let archive = temp.path().join("escape.zip");
    write_zip(&archive, &[("../joy.png", b"image")]);
    assert!(repository.upload_pack(&set, &archive).await.is_err());
    assert!(!characters.join("Alice").exists());

    let archive = temp.path().join("bomb.zip");
    let compressed = vec![0; 64 * 1024];
    write_zip(&archive, &[("joy.png", &compressed)]);
    assert!(repository.upload_pack(&set, &archive).await.is_err());
    assert!(!characters.join("Alice").exists());
}
