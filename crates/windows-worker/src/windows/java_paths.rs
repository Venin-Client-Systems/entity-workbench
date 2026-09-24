//! Change only Java's launch spelling; preserve canonical paths for all trust,
//! ACL, input/hash, output and cleanup operations. No namespace fallback exists.
use super::*;
use windows_sys::Win32::System::WindowsProgramming::DRIVE_FIXED;

pub(super) fn launch_text(path: &Path) -> Result<String> {
    let source = path
        .to_str()
        .ok_or(Error::Blocked("Java launch path encoding rejected"))?;
    let spelling = crate::java::paths::dos_spelling(source)?;
    let drive = wide(&spelling[..3])?;
    // The NUL-terminated drive string is live for this synchronous query. A
    // local fixed disk is required; mapped network/unknown/removable roots fail.
    blocked(
        unsafe { GetDriveTypeW(drive.as_ptr()) } == DRIVE_FIXED,
        "Java launch drive is not local fixed storage",
    )?;
    // Reject reparses on the target and all parents before comparing spellings.
    // Calls occur before the worker runs. Existing copied assets remain subject
    // to their complete inventory and read-only ACL checks after this conversion.
    for ancestor in path.ancestors() {
        ordinary(ancestor)?;
    }
    let canonical = path.canonicalize()?;
    blocked(
        Path::new(&spelling).canonicalize()? == canonical,
        "Java DOS spelling changed canonical identity",
    )?;
    Ok(spelling)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn java_dos_spelling_roundtrips_existing_file_and_directory_identity() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("synthetic.txt");
        fs::write(&file, b"synthetic-only").unwrap();
        for path in [root.path(), file.as_path()] {
            let canonical = path.canonicalize().unwrap();
            let text = launch_text(&canonical).unwrap();
            assert!(!text.contains('?'));
            assert_eq!(Path::new(&text).canonicalize().unwrap(), canonical);
            assert_eq!(launch_text(Path::new(&text)).unwrap(), text);
        }
        assert!(launch_text(&root.path().join("missing")).is_err());
    }
    #[test]
    fn validated_control_paths_are_canonicalized_before_strict_launch_spelling() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("synthetic.txt");
        fs::write(&file, b"synthetic-only").unwrap();
        let normal = launch_text(&root.path().canonicalize().unwrap()).unwrap();
        let supplied = PathBuf::from(format!("{}/./synthetic.txt", normal.replace('\\', "/")));
        assert!(launch_text(&supplied).is_err());
        // Canonicalization is a separate setup action, after validation; the
        // launch helper itself must never silently normalize an unsafe spelling.
        let canonical = supplied.canonicalize().unwrap();
        assert_eq!(canonical, file.canonicalize().unwrap());
        assert_eq!(
            launch_text(&canonical).unwrap(),
            launch_text(&file).unwrap()
        );
    }
}
