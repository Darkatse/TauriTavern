use std::io::Read;
use std::path::PathBuf;

use typed_path::{Utf8WindowsComponent, Utf8WindowsPath};
use zip::read::ZipFile;

use tt_domain::errors::DomainError;

pub(crate) fn enclosed_zip_entry_path<R: Read + ?Sized>(
    entry: &ZipFile<'_, R>,
) -> Result<PathBuf, DomainError> {
    let raw_name = entry.name_raw();
    if raw_name.contains(&0) {
        return Err(DomainError::InvalidData(format!(
            "Invalid archive entry path (NUL byte): {}",
            entry.name()
        )));
    }

    let name = std::str::from_utf8(raw_name).unwrap_or_else(|_| entry.name());
    enclosed_name_from_str(name)
        .ok_or_else(|| DomainError::InvalidData(format!("Invalid archive entry path: {}", name)))
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
