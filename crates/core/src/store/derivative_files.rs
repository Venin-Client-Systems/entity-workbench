//! Private content-addressed derivatives. Unreferenced immutable files may survive failed publication.
use super::*;
use crate::docx_snapshot::ReportArtifactRef;
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
/// Closed storage boundary: reference formats keep their own versioned validators.
#[derive(Clone)]
pub(super) enum ObjectRef {
    Ocr(DerivativeRef),
    Report(ReportArtifactRef),
}
impl ObjectRef {
    pub(super) fn sha256(&self) -> &str {
        match self {
            Self::Ocr(r) => &r.sha256,
            Self::Report(r) => &r.sha256,
        }
    }
    pub(super) fn bytes(&self) -> u64 {
        match self {
            Self::Ocr(r) => r.bytes,
            Self::Report(r) => r.bytes,
        }
    }
    fn validate(&self) -> Result<()> {
        match self {
            Self::Ocr(r) => validate_ref(r),
            Self::Report(r) => r.validate(),
        }
    }
}
pub(super) fn read(root: &Path, reference: &DerivativeRef) -> Result<Vec<u8>> {
    read_object(root, &ObjectRef::Ocr(reference.clone()))
}
pub(super) fn retain(root: &Path, reference: &DerivativeRef, bytes: &[u8]) -> Result<()> {
    retain_object(root, &ObjectRef::Ocr(reference.clone()), bytes)
}
pub(super) fn catalog(conn: &Connection, reference: &DerivativeRef) -> Result<()> {
    catalog_object(conn, &ObjectRef::Ocr(reference.clone()))
}
pub(super) fn verify_catalog(conn: &Connection, reference: &DerivativeRef) -> Result<()> {
    verify_object_catalog(conn, &ObjectRef::Ocr(reference.clone()))
}
fn ordinary(metadata: &fs::Metadata, reference: &ObjectRef) -> Result<()> {
    require(
        metadata.is_file() && !is_link(metadata) && metadata.len() == reference.bytes(),
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
    #[cfg(windows)]
    require(
        metadata.permissions().readonly(),
        "Derivative is not read-only",
    )?;
    Ok(())
}
pub(super) fn read_object(root: &Path, reference: &ObjectRef) -> Result<Vec<u8>> {
    read_checked(root, reference, |_| Ok(()), |_| Ok(()))
}
fn read_checked(
    root: &Path,
    reference: &ObjectRef,
    before_open: impl FnOnce(&Path) -> Result<()>,
    after_open: impl FnOnce(&Path) -> Result<()>,
) -> Result<Vec<u8>> {
    reference.validate()?;
    let path = root.join("derivatives/objects").join(reference.sha256());
    reject_link_ancestors(&path)?;
    let before = fs::symlink_metadata(&path)?;
    ordinary(&before, reference)?;
    before_open(&path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000).share_mode(0x00000001); // Reparse-point handle; read sharing only.
    }
    let mut file = options.open(&path)?;
    let opened = file.metadata()?;
    ordinary(&opened, reference)?;
    super::file_identity::unchanged(&before, &opened)?;
    #[cfg(windows)]
    let identity = super::file_identity::windows_handle(&file, 1)?;
    after_open(&path)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(reference.bytes() + 1)
        .read_to_end(&mut bytes)?;
    require(
        bytes.len() as u64 == reference.bytes() && hash(&bytes) == reference.sha256(),
        "Derivative checksum mismatch",
    )?;
    let finished = file.metadata()?;
    ordinary(&finished, reference)?;
    super::file_identity::unchanged(&opened, &finished)?;
    reject_link_ancestors(&path)?;
    let named = fs::symlink_metadata(&path)?;
    ordinary(&named, reference)?;
    super::file_identity::unchanged(&opened, &named)?;
    #[cfg(windows)]
    {
        require(
            identity == super::file_identity::windows_handle(&file, 1)?,
            "Derivative handle changed during read",
        )?;
        let named_handle = options.open(&path)?;
        require(
            identity == super::file_identity::windows_handle(&named_handle, 1)?,
            "Derivative named identity changed during read",
        )?;
    }
    Ok(bytes)
}

/// Prepare complete files before the canonical transaction. Never replace existing digest names.
pub(super) fn retain_object(root: &Path, reference: &ObjectRef, bytes: &[u8]) -> Result<()> {
    reference.validate()?;
    require(
        bytes.len() as u64 == reference.bytes() && hash(bytes) == reference.sha256(),
        "Derivative bytes do not match their reference",
    )?;
    let directory = root.join("derivatives/objects");
    private_dir(&directory)?;
    let target = directory.join(reference.sha256());
    match fs::symlink_metadata(&target) {
        Ok(_) => {
            read_object(root, reference)?;
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
    read_object(root, reference)?;
    Ok(())
}
pub(super) fn catalog_object(conn: &Connection, reference: &ObjectRef) -> Result<()> {
    reference.validate()?;
    conn.execute(
        "INSERT OR IGNORE INTO derivative_objects(sha256,bytes) VALUES(?,?)",
        params![reference.sha256(), reference.bytes()],
    )?;
    verify_object_catalog(conn, reference)
}
pub(super) fn verify_object_catalog(conn: &Connection, reference: &ObjectRef) -> Result<()> {
    let bytes: u64 = conn.query_row(
        "SELECT bytes FROM derivative_objects WHERE sha256=?",
        [&reference.sha256()],
        |row| row.get(0),
    )?;
    require(
        bytes == reference.bytes(),
        "Derivative catalog length conflicts with reference",
    )
}

#[cfg(test)]
mod tests;
