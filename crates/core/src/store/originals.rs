//! One bounded read supplies both original verification and its verified bytes.
use super::*;
use std::io::Read;

fn ordinary(metadata: &fs::Metadata, evidence: &Evidence) -> Result<()> {
    require(
        metadata.is_file() && !is_link(metadata) && metadata.len() == evidence.bytes,
        "Missing, linked, special or altered original evidence",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        require(
            metadata.nlink() == 1,
            "Hard-linked original evidence rejected",
        )?;
    }
    Ok(())
}
fn same_file(before: &fs::Metadata, after: &fs::Metadata) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        require(
            before.dev() == after.dev()
                && before.ino() == after.ino()
                && before.ctime() == after.ctime()
                && before.ctime_nsec() == after.ctime_nsec(),
            "Original file identity or metadata changed during read",
        )?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // The open handle denies write/delete sharing throughout the read. These
        // checks additionally refuse a metadata mismatch at the named path.
        require(
            before.creation_time() == after.creation_time()
                && before.last_write_time() == after.last_write_time()
                && before.file_attributes() == after.file_attributes(),
            "Original file metadata changed during read",
        )?;
    }
    require(
        before.len() == after.len() && before.modified()? == after.modified()?,
        "Original file changed during read",
    )
}
pub(super) fn read_original(root: &Path, evidence: &Evidence) -> Result<Vec<u8>> {
    read_checked(root, evidence, |_| Ok(()))
}
fn read_checked(
    root: &Path,
    evidence: &Evidence,
    after_open: impl FnOnce(&Path) -> Result<()>,
) -> Result<Vec<u8>> {
    read_path_checked(
        &root.join("originals").join(&evidence.sha256),
        evidence,
        after_open,
    )
}
fn validate_reference(evidence: &Evidence) -> Result<()> {
    require(
        evidence.id == evidence.sha256
            && evidence.sha256.len() == 64
            && evidence
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Canonical evidence identity does not match its original digest",
    )?;
    // Empty complete HTTP bodies are valid; ordinary import still rejects empties.
    require(
        evidence.bytes <= policy::MAX_IMPORT_BYTES as u64,
        "Original exceeds import policy size bound",
    )?;
    Ok(())
}
fn read_path_checked(
    path: &Path,
    evidence: &Evidence,
    after_open: impl FnOnce(&Path) -> Result<()>,
) -> Result<Vec<u8>> {
    validate_reference(evidence)?;
    reject_link_ancestors(path)?;
    let before = fs::symlink_metadata(path)?;
    ordinary(&before, evidence)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // NONBLOCK prevents a replacement FIFO from hanging before its type check.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000).share_mode(0x00000001);
        // FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ only: no write/delete sharing.
    }
    let mut file = options.open(path)?;
    let opened = file.metadata()?;
    ordinary(&opened, evidence)?;
    same_file(&before, &opened)?;
    after_open(path)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(evidence.bytes + 1)
        .read_to_end(&mut bytes)?;
    require(
        bytes.len() as u64 == evidence.bytes && hash(&bytes) == evidence.sha256,
        "Original evidence checksum mismatch or altered length",
    )?;
    let finished = file.metadata()?;
    ordinary(&finished, evidence)?;
    same_file(&opened, &finished)?;
    reject_link_ancestors(path)?;
    let named = fs::symlink_metadata(path)?;
    ordinary(&named, evidence)?;
    same_file(&opened, &named)?;
    Ok(bytes)
}

/// Copy only the already verified bounded buffer. Destinations are internal
/// backup/export paths, never frontend-supplied paths; existing names are refused.
pub(super) fn write_verified_copy(path: &Path, evidence: &Evidence, bytes: &[u8]) -> Result<()> {
    validate_reference(evidence)?;
    require(
        bytes.len() as u64 == evidence.bytes && hash(bytes) == evidence.sha256,
        "Copy buffer differs from verified original",
    )?;
    reject_link_ancestors(path)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o400))?;
    }
    file.sync_all()?;
    // Close the writing handle before the Windows read denies write sharing.
    drop(file);
    read_path_checked(path, evidence, |_| Ok(()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    fn specimen() -> (TempDir, Workspace, Evidence) {
        let temp = TempDir::new_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut workspace = Workspace::open(temp.path().join("workspace")).unwrap();
        let key = workspace.import("synthetic.txt", b"original").unwrap();
        let evidence = get(&workspace.conn, "evidence", &key).unwrap();
        (temp, workspace, evidence)
    }
    #[test]
    fn exact_original_and_import_ceiling_and_corruption_are_checked() {
        let (_temp, w, evidence) = specimen();
        assert_eq!(read_original(&w.root, &evidence).unwrap(), b"original");
        let mut oversized = evidence.clone();
        oversized.bytes = policy::MAX_IMPORT_BYTES as u64 + 1;
        assert!(read_original(&w.root, &oversized)
            .unwrap_err()
            .to_string()
            .contains("size bound"));
        let path = w.root.join("originals").join(&evidence.sha256);
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"altered!").unwrap();
        assert!(read_original(&w.root, &evidence).is_err());
        fs::write(&path, b"original and extra").unwrap();
        assert!(read_original(&w.root, &evidence).is_err());
        fs::remove_file(&path).unwrap();
        assert!(read_original(&w.root, &evidence).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn linked_special_and_growing_open_originals_fail_closed() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let (temp, w, evidence) = specimen();
        let path = w.root.join("originals").join(&evidence.sha256);
        let outside = temp.path().join("outside");
        fs::write(&outside, b"original").unwrap();
        fs::remove_file(&path).unwrap();
        symlink(&outside, &path).unwrap();
        assert!(read_original(&w.root, &evidence).is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&outside, &path).unwrap();
        assert!(read_original(&w.root, &evidence).is_err());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(read_original(&w.root, &evidence).is_err());
        fs::remove_dir(&path).unwrap();
        fs::write(&path, b"original").unwrap();
        let mut oversized_read = false;
        let result = read_checked(&w.root, &evidence, |path| {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            let mut output = OpenOptions::new().append(true).open(path)?;
            output.write_all(&vec![b'x'; 2 * 1024 * 1024])?;
            oversized_read = true;
            Ok(())
        });
        assert!(oversized_read);
        assert!(result.is_err());
    }
    #[cfg(unix)]
    #[test]
    fn replaced_path_after_open_is_refused_even_with_identical_bytes() {
        let (_temp, w, evidence) = specimen();
        assert!(read_checked(&w.root, &evidence, |path| {
            fs::remove_file(path)?;
            fs::write(path, b"original")?;
            Ok(())
        })
        .is_err());
    }
    #[cfg(windows)]
    #[test]
    fn open_original_denies_write_and_delete_sharing() {
        let (_temp, w, evidence) = specimen();
        let result = read_checked(&w.root, &evidence, |path| {
            require(
                OpenOptions::new().write(true).open(path).is_err(),
                "Original opened for conflicting write",
            )?;
            require(
                fs::remove_file(path).is_err(),
                "Original was deleted while read handle open",
            )
        });
        assert_eq!(result.unwrap(), b"original");
    }
    #[test]
    fn verified_buffer_copy_survives_source_replacement_without_reopening_it() {
        let (temp, w, evidence) = specimen();
        let bytes = read_original(&w.root, &evidence).unwrap();
        let source = w.root.join("originals").join(&evidence.sha256);
        fs::remove_file(&source).unwrap();
        fs::write(&source, b"replaced").unwrap();
        assert!(read_original(&w.root, &evidence).is_err());
        let target = temp.path().join("retained-original.bin");
        write_verified_copy(&target, &evidence, &bytes).unwrap();
        assert_eq!(
            read_path_checked(&target, &evidence, |_| Ok(())).unwrap(),
            b"original"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o400
            );
        }
        assert_eq!(fs::read(&source).unwrap(), b"replaced");
    }
    #[test]
    fn copy_rejects_bad_buffers_and_existing_names_and_accepts_empty_original() {
        let (temp, _w, evidence) = specimen();
        let target = temp.path().join("copy.bin");
        assert!(write_verified_copy(&target, &evidence, b"replaced").is_err());
        assert!(!target.exists());
        let mut oversized = evidence.clone();
        oversized.bytes = policy::MAX_IMPORT_BYTES as u64 + 1;
        assert!(write_verified_copy(&target, &oversized, b"original").is_err());
        assert!(!target.exists());
        fs::write(&target, b"preserve existing").unwrap();
        assert!(write_verified_copy(&target, &evidence, b"original").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"preserve existing");
        let empty_target = temp.path().join("empty.bin");
        let mut empty = evidence;
        empty.id = hash(b"");
        empty.sha256 = empty.id.clone();
        empty.bytes = 0;
        write_verified_copy(&empty_target, &empty, b"").unwrap();
        assert!(read_path_checked(&empty_target, &empty, |_| Ok(()))
            .unwrap()
            .is_empty());
    }
    #[cfg(unix)]
    #[test]
    fn copy_refuses_linked_destination_and_preserves_external_file() {
        use std::os::unix::fs::symlink;
        let (temp, _w, evidence) = specimen();
        let outside = temp.path().join("outside");
        fs::write(&outside, b"external sentinel").unwrap();
        let target = temp.path().join("linked.bin");
        symlink(&outside, &target).unwrap();
        assert!(write_verified_copy(&target, &evidence, b"original").is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"external sentinel");
    }
}
