use std::io::Read;
use std::path::PathBuf;

use typed_path::{Utf8WindowsComponent, Utf8WindowsPath};
use zip::read::ZipFile;

use crate::domain::errors::DomainError;

pub fn enclosed_zip_entry_path<R: Read + ?Sized>(
    entry: &ZipFile<'_, R>,
) -> Result<PathBuf, DomainError> {
    Ok(enclosed_zip_entry_path_with_name(entry)?.0)
}

pub fn enclosed_zip_entry_path_with_name<'a, 'b, R: Read + ?Sized>(
    entry: &'b ZipFile<'a, R>,
) -> Result<(PathBuf, &'b str), DomainError> {
    let name = zip_entry_display_name(entry)?;
    let path = enclosed_name_from_str(name).ok_or_else(|| {
        DomainError::InvalidData(format!("Invalid archive entry path: {}", name))
    })?;
    Ok((path, name))
}

pub fn zip_entry_display_name<'a, 'b, R: Read + ?Sized>(
    entry: &'b ZipFile<'a, R>,
) -> Result<&'b str, DomainError> {
    let raw_name = entry.name_raw();
    if raw_name.contains(&0) {
        return Err(DomainError::InvalidData(format!(
            "Invalid archive entry path (NUL byte): {}",
            entry.name()
        )));
    }

    // Some ZIP writers store UTF-8 names without setting the UTF-8 flag.
    // In that case the zip crate decodes `entry.name()` as CP437, producing mojibake
    // (e.g. Chinese becomes `σñÅ...`). Prefer strict UTF-8 when possible.
    Ok(std::str::from_utf8(raw_name).unwrap_or_else(|_| entry.name()))
}

fn enclosed_name_from_str(name: &str) -> Option<PathBuf> {
    if name.contains('\0') {
        return None;
    }

    let mut depth = 0usize;
    let mut out_path = PathBuf::new();
    for component in Utf8WindowsPath::new(name).components() {
        match component {
            Utf8WindowsComponent::Prefix(_) | Utf8WindowsComponent::RootDir => {
                if depth > 0 {
                    return None;
                }
            }
            Utf8WindowsComponent::ParentDir => {
                depth = depth.checked_sub(1)?;
                out_path.pop();
            }
            Utf8WindowsComponent::Normal(segment) => {
                depth += 1;
                out_path.push(segment);
            }
            Utf8WindowsComponent::CurDir => (),
        }
    }

    Some(out_path)
}
