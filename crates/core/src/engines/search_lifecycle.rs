//! Search-only index ownership. A surviving intent is never automatic recovery authority.
// The ownership implementation is compiled for portable tests, but native Search remains macOS-only.
#![cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
use crate::{require, Error, Result};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub(crate) const INTENT: &str = "execution-intent.v1.json";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Disposition {
    Released,
    RecoveryRequired,
}
pub(crate) struct Completion<T> {
    pub(crate) result: Result<T>,
    pub(crate) disposition: Disposition,
}
impl<T> Completion<T> {
    pub(crate) fn released(result: Result<T>) -> Self {
        Self {
            result,
            disposition: Disposition::Released,
        }
    }
    fn recovery(error: Error) -> Self {
        Self {
            result: Err(error),
            disposition: Disposition::RecoveryRequired,
        }
    }
}

fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}
fn ancestors(path: &Path) -> Result<()> {
    for node in path.ancestors() {
        match fs::symlink_metadata(node) {
            Ok(meta) => require(
                !linked(&meta),
                "Search path contains a link or reparse point",
            )?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
/// Presence is enough to refuse execution. No parse, PID lookup or revision can clear an intent.
/// Indeterminate/linked paths also require recovery, including on ordinary coordinator startup.
pub(crate) fn recovery_required(cache: &Path) -> bool {
    ancestors(cache).is_err()
        || !matches!(fs::symlink_metadata(cache.join(INTENT)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}
fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
}
#[derive(PartialEq, Eq)]
struct Identity {
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    volume: u64,
    #[cfg(windows)]
    index: [u8; 16],
}
fn identity(file: &File) -> Result<Identity> {
    node_identity(file, true)
}
fn node_identity(file: &File, ordinary_file: bool) -> Result<Identity> {
    let meta = file.metadata()?;
    require(
        (if ordinary_file {
            meta.is_file()
        } else {
            meta.is_dir()
        }) && !linked(&meta),
        "Invalid search control file",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        require(
            !ordinary_file || meta.nlink() == 1,
            "Hardlinked search control file",
        )?;
        Ok(Identity {
            dev: meta.dev(),
            inode: meta.ino(),
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::*;
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the owned File and correctly sized output structures remain live for each call.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        require(
            !ordinary_file || info.nNumberOfLinks == 1,
            "Hardlinked search control file",
        )?;
        let mut full: FILE_ID_INFO = unsafe { std::mem::zeroed() };
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut full as *mut FILE_ID_INFO).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Identity {
            volume: full.VolumeSerialNumber,
            index: full.FileId.Identifier,
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(Error::Blocked(
            "Search control-file identity is unsupported".into(),
        ))
    }
}
struct OwnedFile {
    path: PathBuf,
    file: File,
    identity: Identity,
}
impl OwnedFile {
    fn create(path: PathBuf) -> Result<Self> {
        let file = options().create_new(true).open(&path)?;
        let identity = identity(&file)?;
        Ok(Self {
            path,
            file,
            identity,
        })
    }
    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.file.write_all(bytes)?;
        self.file.sync_all()?;
        Ok(())
    }
    fn remove(&self) -> Result<()> {
        ancestors(&self.path)?;
        let named = options().open(&self.path)?;
        require(
            identity(&named)? == self.identity && identity(&self.file)? == self.identity,
            "Search control-file identity changed",
        )?;
        fs::remove_file(&self.path)?;
        Ok(())
    }
}
fn open_directory(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    Ok(options.open(path)?)
}
fn sync_directory(directory: &File) -> Result<()> {
    #[cfg(unix)]
    directory.sync_all()?;
    // Native Search remains macOS-only. Other targets exercise synthetic lifecycle tests only.
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}
#[derive(Serialize)]
struct Intent {
    format_version: u32,
    attempt_id: String,
    operation: &'static str,
    workspace_revision: u64,
}
pub(super) struct Lease {
    cache: PathBuf,
    directory: File,
    directory_identity: Identity,
    lock: Option<File>,
    intent: Option<OwnedFile>,
    inputs: Vec<OwnedFile>,
    released: bool,
    uncertain_ownership: bool,
}
impl Lease {
    pub(super) fn acquire(cache: &Path) -> std::result::Result<Self, Completion<()>> {
        Self::acquire_inner(cache).map_err(Completion::recovery)
    }
    fn acquire_inner(cache: &Path) -> Result<Self> {
        ancestors(cache)?;
        fs::create_dir_all(cache)?;
        ancestors(cache)?;
        let cache = cache.canonicalize()?;
        let directory = open_directory(&cache)?;
        let directory_identity = node_identity(&directory, false)?;
        require(
            fs::symlink_metadata(&cache)?.is_dir(),
            "Invalid search cache",
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            directory.set_permissions(fs::Permissions::from_mode(0o700))?;
        }
        let lock = options()
            .create(true)
            .truncate(false)
            .open(cache.join("coordinator.lock"))?;
        identity(&lock)?;
        lock.try_lock().map_err(|_| {
            Error::Blocked("Local index is already owned or requires recovery".into())
        })?;
        let lease = Self {
            cache,
            directory,
            directory_identity,
            lock: Some(lock),
            intent: None,
            inputs: Vec::new(),
            released: false,
            uncertain_ownership: false,
        };
        if recovery_required(&lease.cache) {
            return Err(Error::Blocked(
                "Local index requires verified execution recovery".into(),
            ));
        }
        Ok(lease)
    }
    pub(super) fn cache(&self) -> &Path {
        &self.cache
    }
    fn check_directory(&self) -> Result<()> {
        ancestors(&self.cache)?;
        require(
            node_identity(&open_directory(&self.cache)?, false)? == self.directory_identity
                && node_identity(&self.directory, false)? == self.directory_identity,
            "Search directory identity changed",
        )
    }
    pub(super) fn prepare(&mut self, revision: u64) -> Result<()> {
        self.check_directory()?;
        let bytes = serde_json::to_vec(&Intent {
            format_version: 1,
            attempt_id: uuid::Uuid::new_v4().to_string(),
            operation: "index_then_search_v1",
            workspace_revision: revision,
        })?;
        self.uncertain_ownership = true;
        self.intent = Some(OwnedFile::create(self.cache.join(INTENT))?);
        self.uncertain_ownership = false;
        #[cfg(test)]
        hooks::checkpoint("intent_write")?;
        self.intent.as_mut().expect("owned intent").write(&bytes)?;
        #[cfg(test)]
        hooks::checkpoint("intent_sync")?;
        sync_directory(&self.directory)
    }
    pub(super) fn stage(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        require(
            !name.is_empty() && !name.contains(['/', '\\']) && !matches!(name, "." | ".."),
            "Invalid internal search input name",
        )?;
        self.uncertain_ownership = true;
        let input = OwnedFile::create(self.cache.join(name))?;
        self.inputs.push(input);
        self.uncertain_ownership = false;
        #[cfg(test)]
        hooks::checkpoint("input_write")?;
        self.inputs.last_mut().expect("owned input").write(bytes)
    }
    pub(super) fn finish<T>(mut self, result: Result<T>) -> Completion<T> {
        // Neither assigned inputs nor intent/index may be inspected after unknown exit.
        if self.uncertain_ownership
            || matches!(
                result,
                Err(Error::TerminationUnverified(_) | Error::Cleanup(_))
            )
        {
            return Completion {
                result,
                disposition: Disposition::RecoveryRequired,
            };
        }
        let cleanup = (|| {
            self.check_directory()?;
            #[cfg(test)]
            hooks::checkpoint("input_cleanup")?;
            for input in &self.inputs {
                input.remove()?;
            }
            if let Some(intent) = &self.intent {
                #[cfg(test)]
                hooks::checkpoint("intent_remove")?;
                intent.remove()?;
            }
            #[cfg(test)]
            hooks::checkpoint("finish_sync")?;
            sync_directory(&self.directory)?;
            #[cfg(test)]
            hooks::checkpoint("lock_release")?;
            self.lock.as_ref().expect("owned lock").unlock()?;
            Ok(())
        })();
        match cleanup {
            Ok(()) => {
                self.released = true;
                Completion::released(result)
            }
            Err(error) => Completion::recovery(cleanup_error(result, error)),
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if !self.released {
            // At most one retained lock per affected cache; durable intent survives process loss.
            if let Some(lock) = self.lock.take() {
                std::mem::forget(lock);
            }
        }
    }
}
pub(super) fn cleanup_error<T>(result: Result<T>, cleanup: Error) -> Error {
    let preceding = result
        .err()
        .map_or_else(|| "worker completed".into(), |error| error.to_string());
    Error::Cleanup(format!(
        "Search cleanup failed; result rejected ({preceding}; cleanup: {cleanup})"
    ))
}

#[cfg(test)]
pub(crate) mod hooks {
    use super::*;
    use std::cell::RefCell;
    thread_local! { static STATE: RefCell<(Option<&'static str>, Vec<&'static str>)> = const { RefCell::new((None, Vec::new())) }; }
    pub(crate) fn fail_at(step: &'static str) {
        STATE.with(|state| *state.borrow_mut() = (Some(step), Vec::new()));
    }
    pub(crate) fn take() -> Vec<&'static str> {
        STATE.with(|state| std::mem::take(&mut *state.borrow_mut()).1)
    }
    pub(super) fn checkpoint(step: &'static str) -> Result<()> {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.1.push(step);
            if state.0 == Some(step) {
                state.0 = None;
                Err(std::io::Error::other("synthetic search lifecycle failure").into())
            } else {
                Ok(())
            }
        })
    }
}
#[cfg(test)]
#[path = "search_lifecycle_tests.rs"]
mod tests;
