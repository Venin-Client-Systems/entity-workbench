//! Bounded copies of staged synthetic runtime controls, never user case files.
use super::{check, Failure, ProbeResult};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

pub(super) fn copy(source: &Path, destination: &Path) -> ProbeResult<()> {
    check(!destination.exists(), Failure::RuntimeControl)?;
    fs::create_dir(destination).map_err(|_| Failure::RuntimeControl)?;
    let mut count = 0usize;
    let mut total = 0u64;
    visit(source, destination, 0, &mut count, &mut total)
}
fn ordinary(path: &Path) -> ProbeResult<fs::Metadata> {
    let metadata = fs::symlink_metadata(path).map_err(|_| Failure::RuntimeControl)?;
    check(
        !metadata.is_symlink() && (metadata.is_dir() || metadata.is_file()),
        Failure::RuntimeControl,
    )?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        check(
            metadata.file_attributes() & 0x400 == 0,
            Failure::RuntimeControl,
        )?;
    }
    Ok(metadata)
}
fn pin(path: &Path, directory: bool) -> ProbeResult<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Hold each ancestor through traversal. Deny concurrent write/delete and
        // open the final object itself, without following a reparse target.
        options
            .share_mode(1)
            .custom_flags(0x00200000 | if directory { 0x02000000 } else { 0 });
    }
    let file = options.open(path).map_err(|_| Failure::RuntimeControl)?;
    let metadata = file.metadata().map_err(|_| Failure::RuntimeControl)?;
    check(
        metadata.is_dir() == directory && (directory || metadata.is_file()),
        Failure::RuntimeControl,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        check(directory || metadata.nlink() == 1, Failure::RuntimeControl)?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
        };
        let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // Owned live handle and initialized output buffer cover this synchronous call.
        check(
            unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } != 0
                && information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
                && (directory || information.nNumberOfLinks == 1),
            Failure::RuntimeControl,
        )?;
    }
    Ok(file)
}
fn visit(
    source: &Path,
    target: &Path,
    depth: usize,
    count: &mut usize,
    total: &mut u64,
) -> ProbeResult<()> {
    check(depth <= 16, Failure::RuntimeControl)?;
    let _parent = pin(source, true)?;
    for entry in fs::read_dir(source).map_err(|_| Failure::RuntimeControl)? {
        let entry = entry.map_err(|_| Failure::RuntimeControl)?;
        *count += 1;
        check(*count <= 10000, Failure::RuntimeControl)?;
        let path = entry.path();
        let destination = target.join(entry.file_name());
        if ordinary(&path)?.is_dir() {
            fs::create_dir(&destination).map_err(|_| Failure::RuntimeControl)?;
            visit(&path, &destination, depth + 1, count, total)?;
        } else {
            let mut input = pin(&path, false)?;
            let length = input.metadata().map_err(|_| Failure::RuntimeControl)?.len();
            *total = total.checked_add(length).ok_or(Failure::RuntimeControl)?;
            check(*total <= 1024 * 1024 * 1024, Failure::RuntimeControl)?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
                .map_err(|_| Failure::RuntimeControl)?;
            let mut copied = 0u64;
            let mut buffer = [0u8; 65536];
            loop {
                let size = input
                    .read(&mut buffer)
                    .map_err(|_| Failure::RuntimeControl)?;
                if size == 0 {
                    break;
                }
                copied += size as u64;
                check(copied <= length, Failure::RuntimeControl)?;
                output
                    .write_all(&buffer[..size])
                    .map_err(|_| Failure::RuntimeControl)?;
            }
            check(copied == length, Failure::RuntimeControl)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_copy_only_ordinary_bounded_owned_files() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("asset"), b"synthetic").unwrap();
        let destination = root.path().join("copy");
        copy(&source, &destination).unwrap();
        assert_eq!(fs::read(destination.join("asset")).unwrap(), b"synthetic");
        assert!(copy(&source, &destination).is_err());
        fs::hard_link(source.join("asset"), root.path().join("outside")).unwrap();
        assert!(copy(&source, &root.path().join("linked-copy")).is_err());
    }
}
