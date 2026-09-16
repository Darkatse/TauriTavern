use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path};

use serde_json::Value;
use tt_adapter_storage_core::file_system::{
    persist_file_blocking, persist_json_file_blocking, unique_temp_path,
};
use tt_adapter_storage_core::png_metadata::{
    PNG_SIGNATURE, read_preferred_text_chunk, replace_text_chunks, write_utf8_text_chunk,
};
use tt_domain::errors::DomainError;
use tt_domain::models::persona::{Persona, Personas, take_personas};

const KEYWORD: &str = "persona";
const DEFAULT_AVATAR: &[u8] = include_bytes!("../../../../default/content/user-default.png");

pub(crate) fn avatar_path(directory: &Path, id: &str) -> Result<std::path::PathBuf, DomainError> {
    let mut components = Path::new(id).components();
    if id.is_empty()
        || id.contains(['/', '\\'])
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(DomainError::InvalidData(format!(
            "Invalid Persona avatar: {id}"
        )));
    }
    Ok(directory.join(id))
}

fn parse(text: &str) -> Result<Persona, DomainError> {
    serde_json::from_str(text)
        .map_err(|error| DomainError::InvalidData(format!("Invalid Persona metadata: {error}")))
}

pub(crate) fn from_image(image: &[u8]) -> Result<Option<Persona>, DomainError> {
    if !image.starts_with(&PNG_SIGNATURE) {
        return Ok(None);
    }
    read_preferred_text_chunk(Cursor::new(image), &[KEYWORD])?
        .map(|chunk| parse(&chunk.text))
        .transpose()
}

pub(crate) fn with_persona(image: &[u8], persona: &Persona) -> Result<Vec<u8>, DomainError> {
    let normalized;
    let image = if image.starts_with(&PNG_SIGNATURE) {
        image
    } else {
        // Old imports can contain JPEG/WebP bytes under an existing avatar id.
        // Preserve that id and normalize the carrier only when it is written.
        let decoded = image::load_from_memory(image)
            .map_err(|error| DomainError::InvalidData(format!("Invalid Persona image: {error}")))?;
        let mut buffer = Cursor::new(Vec::new());
        decoded
            .write_to(&mut buffer, image::ImageFormat::Png)
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        normalized = buffer.into_inner();
        &normalized
    };
    let text = serde_json::to_string(persona)
        .map_err(|error| DomainError::InvalidData(error.to_string()))?;
    let mut chunk = Vec::new();
    write_utf8_text_chunk(&mut chunk, KEYWORD, &text);
    replace_text_chunks(image, &[KEYWORD], &chunk)
}

pub(crate) fn publish(path: &Path, bytes: &[u8]) -> Result<(), DomainError> {
    fs::create_dir_all(path.parent().expect("avatar parent")).map_err(io_error)?;
    let temporary = unique_temp_path(path);
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(io_error)?;
        file.write_all(bytes).map_err(io_error)?;
        persist_file_blocking(file, &temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn save(directory: &Path, id: &str, persona: &Persona) -> Result<(), DomainError> {
    let path = avatar_path(directory, id)?;
    let image = match fs::read(&path) {
        Ok(image) => image,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(DomainError::NotFound(format!(
                "Persona {id} no longer exists"
            )));
        }
        Err(error) => return Err(io_error(error)),
    };
    if from_image(&image).is_ok_and(|stored| stored.as_ref() == Some(persona)) {
        return Ok(());
    }
    publish(&path, &with_persona(&image, persona)?)
}

/// Explicit imports may create cards or recover an unusable avatar; ordinary edits never do.
pub(crate) fn import_persona(
    directory: &Path,
    id: &str,
    persona: &Persona,
) -> Result<(), DomainError> {
    let path = avatar_path(directory, id)?;
    let image = match fs::read(&path) {
        Ok(image) => image,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => DEFAULT_AVATAR.to_vec(),
        Err(error) => return Err(io_error(error)),
    };
    if from_image(&image).is_ok_and(|stored| stored.as_ref() == Some(persona)) {
        return Ok(());
    }
    let bytes = match with_persona(&image, persona) {
        Ok(bytes) => bytes,
        Err(DomainError::InvalidData(error)) => {
            let backup = path.with_file_name(format!("{id}.corrupt-{}", uuid::Uuid::new_v4()));
            fs::copy(&path, &backup).map_err(io_error)?;
            let bytes = with_persona(DEFAULT_AVATAR, persona)?;
            publish(&path, &bytes)?;
            tracing::error!(target: tt_contracts::observability::USER_VISIBLE_ERROR,
                "Persona {id}: {error}; original saved at {}; using the default avatar", backup.display());
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    publish(&path, &bytes)
}

/// One PNG is one Persona. A bad card is reported without hiding healthy cards.
pub fn read_personas(user_root: &Path) -> Result<Personas, DomainError> {
    let directory = user_root.join("User Avatars");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Personas::new()),
        Err(error) => return Err(io_error(error)),
    };
    let mut personas = Personas::new();
    for entry in entries {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        if !entry.file_type().map_err(io_error)?.is_file()
            || !mime_guess::from_path(&path)
                .first()
                .is_some_and(|mime| mime.type_() == "image")
        {
            continue;
        }
        let id = entry
            .file_name()
            .into_string()
            .map_err(|_| DomainError::InvalidData("Persona filename is not UTF-8".into()))?;
        match read(&path) {
            Ok(persona) => {
                personas.insert(id, persona);
            }
            Err(error) => {
                tracing::error!(target: tt_contracts::observability::USER_VISIBLE_ERROR, "Failed to read Persona {}: {error}", path.display())
            }
        }
    }
    Ok(personas)
}

fn read(path: &Path) -> Result<Persona, DomainError> {
    let mut file = fs::File::open(path).map_err(io_error)?;
    let mut signature = [0; 8];
    let length = file.read(&mut signature).map_err(io_error)?;
    if length != 8 || signature != PNG_SIGNATURE {
        return Ok(Persona::default());
    }
    read_preferred_text_chunk(file, &[KEYWORD])?
        .map(|chunk| parse(&chunk.text))
        .transpose()
        .map(Option::unwrap_or_default)
}

/// Convert legacy settings at startup or before an archive overlays the target's files.
pub fn migrate_personas(user_root: &Path, target_user_root: &Path) -> Result<(), DomainError> {
    let path = user_root.join("settings.json");
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(error)),
    };
    let mut settings: Value = serde_json::from_slice(&bytes)
        .map_err(|error| DomainError::InvalidData(format!("Invalid Persona settings: {error}")))?;
    let Some(personas) = take_personas(&mut settings)? else {
        return Ok(());
    };
    let directory = user_root.join("User Avatars");
    for (id, persona) in personas {
        let target = avatar_path(&directory, &id)?;
        if !target.exists() {
            let existing = avatar_path(&target_user_root.join("User Avatars"), &id)?;
            match fs::read(existing) {
                Ok(image) => publish(&target, &image)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(error)),
            }
        }
        import_persona(&directory, &id, &persona)?;
    }
    persist_json_file_blocking(&path, &settings)
}

fn io_error(error: std::io::Error) -> DomainError {
    DomainError::InternalError(error.to_string())
}
