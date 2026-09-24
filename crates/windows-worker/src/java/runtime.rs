//! Complete development-runtime inventory. No worker-selected classpaths or assets.
use super::Role;
use crate::{outcomes::check_cancelled, preparation::stream_exact, Error, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::Read, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEntry {
    bytes: u64,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    development_only: bool,
    role: Role,
    java_version: String,
    files: BTreeMap<String, FileEntry>,
    directories: Vec<String>,
}
pub(super) fn component(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    !name.is_empty()
        && name.len() <= 128
        && (name.as_bytes()[0].is_ascii_alphanumeric() || name.as_bytes()[0] == b'_')
        && !name.ends_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}
fn relative(name: &str) -> bool {
    name.len() <= 512 && name.split('/').all(component)
}
fn ordinary(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(Error::Blocked("runtime reparse entry rejected"));
        }
    }
    if metadata.is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
        return Err(Error::Blocked("runtime link or special entry rejected"));
    }
    Ok(metadata)
}
pub(super) fn ordinary_ancestors(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::Blocked("assigned parent must be absolute"));
    }
    for parent in path.ancestors() {
        if !ordinary(parent)?.is_dir() {
            return Err(Error::Blocked(
                "assigned ancestor is not an ordinary directory",
            ));
        }
    }
    Ok(())
}
fn file(path: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x00200000); // READ sharing; OPEN_REPARSE_POINT.
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(Error::Blocked("runtime asset is not an ordinary file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(Error::Blocked("runtime hardlink rejected"));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::*;
        let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // This initialized buffer and the live owned file handle cover the call.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
            return Err(Error::Blocked("runtime file identity unavailable"));
        }
        if information.nNumberOfLinks != 1
            || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(Error::Blocked("runtime linked file rejected"));
        }
        crate::windows::reject_named_streams(path)?;
    }
    Ok(file)
}
pub(super) fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    ordinary(path)?;
    let file = file(path)?;
    if file.metadata()?.len() > maximum {
        return Err(Error::Blocked("runtime asset exceeds bound"));
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err(Error::Blocked("runtime asset grew beyond bound"));
    }
    Ok(bytes)
}
pub(super) fn verify(root: &Path, expected: Role) -> Result<()> {
    verify_with_cancel(root, expected, &|| false)
}
pub(super) fn verify_with_cancel(
    root: &Path,
    expected: Role,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    check_cancelled(cancelled)?;
    ordinary_ancestors(root)?;
    let manifest: Manifest =
        serde_json::from_slice(&read_bounded(&root.join("manifest.json"), 1024 * 1024)?)
            .map_err(|_| Error::Blocked("invalid runtime manifest"))?;
    if manifest.schema_version != 1
        || !manifest.development_only
        || manifest.role != expected
        || !manifest.java_version.starts_with("21.")
        || manifest.files.len() > 10000
        || !manifest.files.contains_key("java/bin/java.exe")
        || !manifest.files.contains_key("worker.jar")
        || manifest.files.contains_key("manifest.json")
    {
        return Err(Error::Blocked(
            "runtime role or inventory contract mismatch",
        ));
    }
    let required = match expected {
        Role::Parser => [
            "lib/tika-core-3.3.2.jar",
            "lib/tika-parser-microsoft-module-3.3.2.jar",
            "lib/pdfbox-3.0.8.jar",
            "lib/jackson-databind-2.22.3.jar",
        ]
        .as_slice(),
        Role::Search => [
            "lib/lucene-core-10.5.1.jar",
            "lib/lucene-analysis-common-10.5.1.jar",
            "lib/lucene-queryparser-10.5.1.jar",
            "lib/jackson-databind-2.22.3.jar",
        ]
        .as_slice(),
    };
    if !required
        .iter()
        .all(|name| manifest.files.contains_key(*name))
        || manifest.files.keys().any(|name| {
            if name == "worker.jar" || name.starts_with("java/") {
                return false;
            }
            let Some(library) = name.strip_prefix("lib/") else {
                return true;
            };
            library.contains('/')
                || !library.ends_with(".jar")
                || match expected {
                    Role::Parser => library.starts_with("lucene-"),
                    Role::Search => {
                        !library.starts_with("lucene-") && !library.starts_with("jackson-")
                    }
                }
        })
        || manifest
            .directories
            .iter()
            .any(|name| name != "lib" && name != "java" && !name.starts_with("java/"))
    {
        return Err(Error::Blocked("runtime assets violate the fixed role"));
    }
    let mut actual_files = Vec::new();
    let mut actual_directories = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut pending = vec![(root.to_path_buf(), 0)];
    let mut total = 0u64;
    while let Some((directory, depth)) = pending.pop() {
        check_cancelled(cancelled)?;
        if depth > 16 {
            return Err(Error::Blocked("runtime depth exceeded"));
        }
        for entry in fs::read_dir(directory)? {
            check_cancelled(cancelled)?;
            let entry = entry?;
            let path = entry.path();
            let name = path
                .strip_prefix(root)
                .map_err(|_| Error::Blocked("runtime path escape"))?
                .to_str()
                .ok_or(Error::Blocked("runtime name encoding rejected"))?
                .replace('\\', "/");
            if !relative(&name) || !seen.insert(name.to_ascii_lowercase()) || seen.len() > 10000 {
                return Err(Error::Blocked("unsafe or colliding runtime inventory"));
            }
            if ordinary(&path)?.is_dir() {
                actual_directories.push(name);
                pending.push((path, depth + 1));
            } else if name == "manifest.json" {
                continue;
            } else {
                let expected = manifest
                    .files
                    .get(&name)
                    .ok_or(Error::Blocked("unlisted runtime asset"))?;
                total = total
                    .checked_add(expected.bytes)
                    .ok_or(Error::Blocked("runtime size overflow"))?;
                if total > 1024 * 1024 * 1024
                    || expected.sha256.len() != 64
                    || !expected
                        .sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err(Error::Blocked("runtime size or digest rejected"));
                }
                let mut source = file(&path)?;
                if source.metadata()?.len() != expected.bytes {
                    return Err(Error::Blocked("runtime size mismatch"));
                }
                let mut digest = Sha256::new();
                stream_exact(&mut source, expected.bytes, cancelled, |bytes| {
                    digest.update(bytes);
                    Ok(())
                })?;
                if format!("{:x}", digest.finalize()) != expected.sha256 {
                    return Err(Error::Blocked("runtime digest mismatch"));
                }
                actual_files.push(name);
            }
        }
    }
    check_cancelled(cancelled)?;
    actual_files.sort();
    actual_directories.sort();
    if actual_files != manifest.files.keys().cloned().collect::<Vec<_>>()
        || actual_directories != manifest.directories
    {
        return Err(Error::Blocked("runtime inventory is incomplete"));
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn copy_asset(
    source: &Path,
    destination: &Path,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    use std::io::Write;
    check_cancelled(cancelled)?;
    let mut source = file(source)?;
    let expected = source.metadata()?.len();
    if expected > 1024 * 1024 * 1024 {
        return Err(Error::Blocked("runtime asset exceeds copy bound"));
    }
    let mut destination = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    stream_exact(&mut source, expected, cancelled, |bytes| {
        destination.write_all(bytes)?;
        Ok(())
    })
}
