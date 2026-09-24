use super::*;
fn fixture() -> (tempfile::TempDir, PathBuf, DerivativeRef) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = temp.path().join("case");
    let reference = reference(
        DerivativeKind::ImageRegionResultJsonV1,
        b"synthetic retained object",
    )
    .unwrap();
    retain(&root, &reference, b"synthetic retained object").unwrap();
    (temp, root, reference)
}
#[test]
fn ordinary_object_remains_readable_and_retain_verifies_existing_identity() {
    let (_temp, root, reference) = fixture();
    assert_eq!(
        read(&root, &reference).unwrap(),
        b"synthetic retained object"
    );
    retain(&root, &reference, b"synthetic retained object").unwrap();
    let outside = root.join("extra-link");
    fs::hard_link(
        root.join("derivatives/objects").join(&reference.sha256),
        &outside,
    )
    .unwrap();
    assert!(read(&root, &reference).is_err());
    assert!(retain(&root, &reference, b"synthetic retained object").is_err());
    assert_eq!(fs::read(outside).unwrap(), b"synthetic retained object");
}
#[cfg(unix)]
#[test]
fn replaced_name_after_open_is_refused_even_when_bytes_are_identical() {
    use std::os::unix::fs::PermissionsExt;
    let (_temp, root, reference) = fixture();
    let result = read_checked(
        &root,
        &reference,
        |_| Ok(()),
        |path| {
            fs::remove_file(path)?;
            fs::write(path, b"synthetic retained object")?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o400))?;
            Ok(())
        },
    );
    assert!(result.is_err());
}
#[cfg(unix)]
#[test]
fn substituted_fifo_does_not_block_before_type_check() {
    use std::os::unix::ffi::OsStrExt;
    let (_temp, root, reference) = fixture();
    let started = std::time::Instant::now();
    let result = read_checked(
        &root,
        &reference,
        |path| {
            fs::remove_file(path)?;
            let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            // The owned synthetic fixture path remains a live C string for this call.
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o400) }, 0);
            Ok(())
        },
        |_| panic!("FIFO must be refused before reading"),
    );
    assert!(result.is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}
#[cfg(unix)]
#[test]
fn same_length_in_place_change_is_refused_even_if_original_bytes_are_restored() {
    use std::os::unix::fs::PermissionsExt;
    let (_temp, root, reference) = fixture();
    assert!(read_checked(
        &root,
        &reference,
        |_| Ok(()),
        |path| {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            fs::write(path, b"synthetic retained object")?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o400))?;
            Ok(())
        }
    )
    .is_err());
}
#[cfg(windows)]
#[test]
fn retained_handle_denies_conflicting_write_and_delete() {
    let (_temp, root, reference) = fixture();
    let bytes = read_checked(
        &root,
        &reference,
        |_| Ok(()),
        |path| {
            require(
                OpenOptions::new().write(true).open(path).is_err(),
                "Conflicting write allowed",
            )?;
            require(fs::remove_file(path).is_err(), "Conflicting delete allowed")
        },
    )
    .unwrap();
    assert_eq!(bytes, b"synthetic retained object");
}
