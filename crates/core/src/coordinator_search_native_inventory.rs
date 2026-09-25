//! Bounded test-only selected Java/Search inventory; no interpreter execution.
use crate::{require, Error, Result};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::{self, File, Metadata, OpenOptions},
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
};

pub(super) type Row = (String, u64, String, bool);
pub(super) fn stable(a: &Metadata, b: &Metadata) -> bool {
    (
        a.dev(),
        a.ino(),
        a.mode(),
        a.nlink(),
        a.len(),
        a.mtime(),
        a.mtime_nsec(),
        a.ctime(),
        a.ctime_nsec(),
    ) == (
        b.dev(),
        b.ino(),
        b.mode(),
        b.nlink(),
        b.len(),
        b.mtime(),
        b.mtime_nsec(),
        b.ctime(),
        b.ctime_nsec(),
    )
}
pub(super) fn ordinary_root(path: &Path) -> Result<()> {
    require(
        path.is_absolute() && path.canonicalize()? == path,
        "Probe path alias",
    )?;
    for ancestor in path.ancestors() {
        require(
            fs::symlink_metadata(ancestor)?.is_dir(),
            "Probe directory ancestor is not ordinary",
        )?;
    }
    Ok(())
}
fn open_at(parent: &File, name: &str, directory: bool) -> Result<File> {
    let name =
        CString::new(name).map_err(|_| Error::Validation("Invalid inventory name".into()))?;
    let flags = libc::O_RDONLY
        | libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | if directory { libc::O_DIRECTORY } else { 0 };
    // SAFETY: parent is retained, name is NUL-terminated; successful descriptor is owned once.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
pub(super) fn file_identity(path: &Path, maximum: u64) -> Result<(u64, u64, u64, String)> {
    ordinary_root(
        path.parent()
            .ok_or_else(|| Error::Validation("Missing parent".into()))?,
    )?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let before = file.metadata()?;
    let (size, digest) = read_hash(&mut file, maximum)?;
    require(
        stable(&before, &fs::symlink_metadata(path)?),
        "Inventory path replaced",
    )?;
    Ok((before.dev(), before.ino(), size, digest))
}
fn read_hash(file: &mut File, maximum: u64) -> Result<(u64, String)> {
    let before = file.metadata()?;
    require(
        before.is_file() && before.nlink() == 1 && before.len() <= maximum,
        "Unsafe or oversized inventory file",
    )?;
    let mut hash = Sha256::new();
    let mut count = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        require(count <= maximum, "Growing inventory file")?;
        hash.update(&buffer[..n]);
    }
    require(
        count == before.len() && stable(&before, &file.metadata()?),
        "Inventory file changed",
    )?;
    Ok((count, format!("{:x}", hash.finalize())))
}
fn name_valid(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() <= 180
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
}
struct Walk {
    rows: Vec<Row>,
    entries: usize,
    bytes: u64,
}
impl Walk {
    fn directory(&mut self, path: &Path, dir: &File, relative: &str, depth: usize) -> Result<()> {
        require(depth <= 8, "Inventory depth exceeded")?;
        let before = dir.metadata()?;
        require(
            before.is_dir() && stable(&before, &fs::symlink_metadata(path)?),
            "Inventory directory replaced",
        )?;
        let mut names = Vec::new();
        for entry in fs::read_dir(path)? {
            self.entries += 1;
            require(self.entries <= 1024, "Inventory entry count exceeded")?;
            let name = entry?
                .file_name()
                .into_string()
                .map_err(|_| Error::Validation("Non-UTF8 inventory name".into()))?;
            require(name_valid(&name), "Invalid inventory member name")?;
            names.push(name);
        }
        names.sort();
        for name in names {
            let child_path = path.join(&name);
            let metadata = fs::symlink_metadata(&child_path)?;
            require(
                metadata.is_dir() || (metadata.is_file() && metadata.nlink() == 1),
                "Inventory link or special entry",
            )?;
            let mut child = open_at(dir, &name, metadata.is_dir())?;
            require(
                stable(&metadata, &child.metadata()?),
                "Inventory entry replaced",
            )?;
            let member = format!("{relative}/{name}");
            if metadata.is_dir() {
                self.directory(&child_path, &child, &member, depth + 1)?;
            } else {
                require(
                    self.bytes + metadata.len() <= 256 * 1024 * 1024,
                    "Inventory aggregate exceeded",
                )?;
                let (size, sha) = read_hash(&mut child, 128 * 1024 * 1024)?;
                self.bytes += size;
                require(
                    self.bytes <= 256 * 1024 * 1024,
                    "Inventory aggregate exceeded",
                )?;
                self.rows
                    .push((member, size, sha, metadata.mode() & 0o111 != 0));
            }
            require(
                stable(&metadata, &fs::symlink_metadata(&child_path)?),
                "Inventory entry changed",
            )?;
        }
        require(
            stable(&before, &dir.metadata()?) && stable(&before, &fs::symlink_metadata(path)?),
            "Inventory directory changed",
        )
    }
}
pub(super) fn runtime(root: &Path) -> Result<(String, Vec<Row>)> {
    ordinary_root(root)?;
    let mut walk = Walk {
        rows: vec![],
        entries: 2,
        bytes: 0,
    };
    for component in ["java", "search"] {
        let path = root.join(component);
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&path)?;
        walk.directory(&path, &file, component, 1)?;
    }
    walk.rows.sort_by(|a, b| a.0.cmp(&b.0));
    for required in ["java/bin/java", "java/release", "search/workers-0.1.0.jar"] {
        require(
            walk.rows
                .iter()
                .any(|r| r.0 == required && r.1 > 0 && (required != "java/bin/java" || r.3)),
            "Incomplete selected runtime",
        )?;
    }
    require(
        walk.rows
            .iter()
            .any(|r| r.0.starts_with("search/lib/") && r.0.ends_with(".jar")),
        "Search dependency JARs missing",
    )?;
    let digest = crate::store::hash(&serde_json::to_vec(&walk.rows)?);
    Ok((digest, walk.rows))
}

#[test]
fn native_search_inventory_is_bounded_and_rejects_links_and_incomplete_trees() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("java/bin")).unwrap();
    fs::create_dir_all(root.join("search/lib")).unwrap();
    assert!(runtime(root).is_err());
    use std::os::unix::fs::{symlink, PermissionsExt};
    for (name, bytes) in [
        ("java/bin/java", b"java".as_slice()),
        ("java/release", b"release"),
        ("search/workers-0.1.0.jar", b"worker"),
        ("search/lib/a.jar", b"dependency"),
    ] {
        fs::write(root.join(name), bytes).unwrap();
    }
    fs::set_permissions(
        root.join("java/bin/java"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let baseline = runtime(root).unwrap();
    assert_eq!(baseline.1.len(), 4);
    fs::write(root.join("search/lib/a.jar"), b"changed").unwrap();
    assert_ne!(baseline.0, runtime(root).unwrap().0);
    symlink(root.join("java/release"), root.join("search/lib/link")).unwrap();
    assert!(runtime(root).is_err());
    fs::remove_file(root.join("search/lib/link")).unwrap();
    fs::hard_link(root.join("java/release"), root.join("search/lib/hardlink")).unwrap();
    assert!(runtime(root).is_err());
}
