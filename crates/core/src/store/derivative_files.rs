//! Private content-addressed derivatives. Unreferenced immutable files may survive failed publication.
use super::*;
use crate::processing::{DerivativeKind, DerivativeRef};
#[cfg(unix)]
use std::fs::File;
use std::io::Read;
pub(super) const CREATE_CATALOG: &str = "CREATE TABLE IF NOT EXISTS derivative_objects(sha256 TEXT PRIMARY KEY NOT NULL, bytes INTEGER NOT NULL CHECK(bytes>=0))";
pub(super) const MAX_RESULT_BYTES: u64 = 8 * 1024 * 1024;
fn limit(kind: &DerivativeKind) -> u64 {
    match kind {
        DerivativeKind::CanonicalPgmV1 => (crate::engines::ocr::MAX_PIXELS + 32) as u64,
        DerivativeKind::OcrTsvV1 => crate::engines::ocr_regions::MAX_TSV_BYTES - 1,
        DerivativeKind::ImageRegionResultJsonV1 => MAX_RESULT_BYTES,
    }
}
pub(super) fn validate_ref(reference: &DerivativeRef) -> Result<()> {
    require(
        reference.sha256.len() == 64
            && reference
                .sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "Invalid derivative digest",
    )?;
    require(
        reference.bytes > 0 && reference.bytes <= limit(&reference.kind),
        "Derivative reference exceeds its type bound",
    )
}
pub(super) fn reference(kind: DerivativeKind, bytes: &[u8]) -> Result<DerivativeRef> {
    let reference = DerivativeRef {
        sha256: hash(bytes),
        bytes: bytes.len() as u64,
        kind,
    };
    validate_ref(&reference)?;
    Ok(reference)
}
fn ordinary(metadata: &fs::Metadata, reference: &DerivativeRef) -> Result<()> {
    require(
        metadata.is_file() && !is_link(metadata) && metadata.len() == reference.bytes,
        "Derivative is missing, linked, special or altered",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        require(
            metadata.permissions().mode() & 0o777 == 0o400,
            "Derivative permissions are not private and read-only",
        )?;
        require(metadata.nlink() == 1, "Hard-linked derivative rejected")?;
    }
    Ok(())
}
pub(super) fn read(root: &Path, reference: &DerivativeRef) -> Result<Vec<u8>> {
    validate_ref(reference)?;
    let path = root.join("derivatives/objects").join(&reference.sha256);
    reject_link_ancestors(&path)?;
    ordinary(&fs::symlink_metadata(&path)?, reference)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT.
    }
    let mut file = options.open(&path)?;
    ordinary(&file.metadata()?, reference)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(reference.bytes + 1)
        .read_to_end(&mut bytes)?;
    require(
        bytes.len() as u64 == reference.bytes && hash(&bytes) == reference.sha256,
        "Derivative checksum mismatch",
    )?;
    ordinary(&file.metadata()?, reference)?;
    reject_link_ancestors(&path)?;
    Ok(bytes)
}

/// Prepare complete files before the canonical transaction. Never replace existing digest names.
pub(super) fn retain(root: &Path, reference: &DerivativeRef, bytes: &[u8]) -> Result<()> {
    validate_ref(reference)?;
    require(
        bytes.len() as u64 == reference.bytes && hash(bytes) == reference.sha256,
        "Derivative bytes do not match their reference",
    )?;
    let directory = root.join("derivatives/objects");
    private_dir(&directory)?;
    let target = directory.join(&reference.sha256);
    match fs::symlink_metadata(&target) {
        Ok(_) => {
            read(root, reference)?;
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut pending = tempfile::Builder::new()
        .prefix(".pending-")
        .tempfile_in(&directory)?;
    let prepared: Result<()> = (|| {
        pending.write_all(bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            pending
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o400))?;
        }
        pending.as_file().sync_all()?;
        reject_link_ancestors(&target)?;
        Ok(())
    })();
    if let Err(error) = prepared {
        if pending.close().is_err() {
            return Err(Error::Cleanup(format!(
                "Derivative staging cleanup failed after {error}"
            )));
        }
        return Err(error);
    }
    match pending.persist_noclobber(&target) {
        Ok(file) => {
            #[cfg(windows)]
            {
                let mut permissions = file.metadata()?.permissions();
                permissions.set_readonly(true);
                file.set_permissions(permissions)?;
            }
            file.sync_all()?;
            drop(file);
            #[cfg(unix)]
            {
                File::open(&directory)?.sync_all()?;
            }
        }
        Err(error) => {
            let cause = error.error;
            error
                .file
                .close()
                .map_err(|_| Error::Cleanup("Derivative staging cleanup failed".into()))?;
            if cause.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(cause.into());
            }
        }
    }
    read(root, reference)?;
    Ok(())
}
pub(super) fn catalog(conn: &Connection, reference: &DerivativeRef) -> Result<()> {
    validate_ref(reference)?;
    conn.execute(
        "INSERT OR IGNORE INTO derivative_objects(sha256,bytes) VALUES(?,?)",
        params![reference.sha256, reference.bytes],
    )?;
    verify_catalog(conn, reference)
}
pub(super) fn verify_catalog(conn: &Connection, reference: &DerivativeRef) -> Result<()> {
    let bytes: u64 = conn.query_row(
        "SELECT bytes FROM derivative_objects WHERE sha256=?",
        [&reference.sha256],
        |row| row.get(0),
    )?;
    require(
        bytes == reference.bytes,
        "Derivative catalog length conflicts with reference",
    )
}
