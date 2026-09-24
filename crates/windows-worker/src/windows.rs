//! Win32 boundary: all pointers borrow live allocations; owned handles are RAII.
//! The created process remains suspended until zero capabilities, identity and Job
//! Object assignment are verified. No errors retry without the AppContainer.
use crate::{quote_argument, validate, Error, Output, Request, Result};
use std::{
    ffi::{c_void, OsStr, OsString},
    fs::{self, OpenOptions},
    io::Read,
    mem::{size_of, zeroed},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, Isolation::*, *},
    Storage::FileSystem::*,
    System::{
        Com::CoTaskMemFree, JobObjects::*, SystemServices::MAXIMUM_ALLOWED, Threading::*,
        WindowsProgramming::PROCESS_CREATION_CHILD_PROCESS_RESTRICTED,
    },
};

const OUTPUT_LIMIT: u64 = 1024 * 1024;
const WRITABLE_LIMIT: u64 = 32 * 1024 * 1024;
const HANDLE_LIMIT: u32 = 512;

fn blocked(condition: bool, reason: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::Blocked(reason))
    }
}
fn api(value: i32, operation: &'static str) -> Result<()> {
    if value != 0 {
        Ok(())
    } else {
        Err(Error::Api {
            operation,
            code: unsafe { GetLastError() },
        })
    }
}
fn hr(value: i32, operation: &'static str) -> Result<()> {
    if value >= 0 {
        Ok(())
    } else {
        Err(Error::Api {
            operation,
            code: value as u32,
        })
    }
}
fn wide(value: impl AsRef<OsStr>) -> Result<Vec<u16>> {
    let mut result: Vec<u16> = value.as_ref().encode_wide().collect();
    blocked(!result.contains(&0), "NUL in native argument")?;
    result.push(0);
    Ok(result)
}
unsafe fn from_wide(value: *const u16) -> OsString {
    let mut length = 0;
    while *value.add(length) != 0 {
        length += 1;
    }
    OsString::from_wide(std::slice::from_raw_parts(value, length))
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn token(process: HANDLE) -> Result<Handle> {
    let mut value = null_mut();
    api(
        unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut value) },
        "OpenProcessToken",
    )?;
    Ok(Handle(value))
}
fn token_info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<Vec<usize>> {
    let mut size = 0;
    unsafe {
        GetTokenInformation(token, class, null_mut(), 0, &mut size);
    }
    blocked(size > 0 && size <= 65536, "invalid token information size")?;
    let mut value = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    api(
        unsafe { GetTokenInformation(token, class, value.as_mut_ptr().cast(), size, &mut size) },
        "GetTokenInformation",
    )?;
    Ok(value)
}
fn sid_string(sid: PSID) -> Result<String> {
    let mut value = null_mut();
    api(
        unsafe { ConvertSidToStringSidW(sid, &mut value) },
        "ConvertSidToStringSid",
    )?;
    let allocation = Local(value.cast());
    let result = unsafe { from_wide(value) }
        .into_string()
        .map_err(|_| Error::Blocked("invalid SID encoding"));
    drop(allocation);
    result
}
fn user_sid() -> Result<String> {
    let token = token(unsafe { GetCurrentProcess() })?;
    let info = token_info(token.0, TokenUser)?;
    sid_string(unsafe { (*(info.as_ptr().cast::<TOKEN_USER>())).User.Sid })
}

/// Applies only to newly staged, coordinator-owned paths, before any worker runs.
/// Protected DACL prevents parent ALL APPLICATION PACKAGES grants leaking inward.
fn acl(path: &Path, user: &str, package: Option<(&str, bool)>) -> Result<()> {
    set_acl(path, user, package, false)
}
fn set_acl(
    path: &Path,
    user: &str,
    package: Option<(&str, bool)>,
    directory_repair: bool,
) -> Result<()> {
    let package_ace = package
        .map(|(sid, writable)| {
            format!(
                "(A;OICI;{};;;{sid})",
                if writable { "0x001301bf" } else { "GRGX" }
            )
        })
        .unwrap_or_default();
    let label = if package.is_some_and(|(_, writable)| writable) {
        "S:(ML;OICI;NW;;;LW)"
    } else {
        ""
    };
    let sddl = wide(format!(
        "D:P(A;OICI;RC;;;OW)(A;OICI;FA;;;SY)(A;OICI;FA;;;{user}){package_ace}{label}"
    ))?;
    let mut descriptor = null_mut();
    api(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            )
        },
        "ConvertSecurityDescriptor",
    )?;
    let descriptor = Local(descriptor);
    let (mut dacl, mut sacl) = (null_mut(), null_mut());
    let (mut present, mut defaulted) = (0, 0);
    api(
        unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) },
        "GetDacl",
    )?;
    blocked(present != 0 && !dacl.is_null(), "empty DACL forbidden")?;
    let mut flags = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
    if !label.is_empty() {
        api(
            unsafe {
                GetSecurityDescriptorSacl(descriptor.0, &mut present, &mut sacl, &mut defaulted)
            },
            "GetLabel",
        )?;
        flags |= LABEL_SECURITY_INFORMATION;
    }
    let error = if directory_repair {
        // MAXIMUM_ALLOWED is deliberate: SetSecurityInfo documents that this
        // prevents automatic child-ACE propagation. Otherwise repairing a
        // parent DACL could mutate an outside file via a worker-created hardlink.
        let directory = OpenOptions::new()
            .access_mode(MAXIMUM_ALLOWED)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        let metadata = directory.metadata()?;
        blocked(
            metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
            "repair target is not an ordinary directory",
        )?;
        unsafe {
            SetSecurityInfo(
                directory.as_raw_handle(),
                SE_FILE_OBJECT,
                flags,
                null_mut(),
                null_mut(),
                dacl,
                sacl,
            )
        }
    } else {
        let name = wide(path)?;
        unsafe {
            SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                flags,
                null_mut(),
                null_mut(),
                dacl,
                sacl,
            )
        }
    };
    if error != 0 {
        return Err(Error::Api {
            operation: "SetPrivateAcl",
            code: error,
        });
    }
    Ok(())
}
fn ordinary(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    blocked(
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "reparse path rejected",
    )?;
    blocked(
        metadata.is_file() || metadata.is_dir(),
        "special path rejected",
    )?;
    Ok(metadata)
}
fn reject_named_streams(path: &Path) -> Result<()> {
    // Query the opened entry itself. A path-based stream search could follow a
    // reparse point installed after inspection, even without delete sharing.
    let guard = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    blocked(
        guard.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "reparse stream target rejected",
    )?;
    // FILE_STREAM_INFO requires 8-byte alignment. The fixed 64 KiB buffer bounds
    // allocation and query size; insufficient capacity rejects rather than grows.
    let mut words = vec![0u64; 8192];
    unsafe {
        SetLastError(ERROR_SUCCESS);
    }
    let success = unsafe {
        GetFileInformationByHandleEx(
            guard.as_raw_handle(),
            FileStreamInfo,
            words.as_mut_ptr().cast(),
            (words.len() * size_of::<u64>()) as u32,
        )
    };
    let code = unsafe { GetLastError() };
    if code == ERROR_HANDLE_EOF {
        return Ok(());
    } // Directory without data streams.
    if success == 0 {
        return Err(Error::Api {
            operation: "InspectFileStreams",
            code,
        });
    }
    let data = unsafe { &*words.as_ptr().cast::<FILE_STREAM_INFO>() };
    // The only accepted record is the unnamed default data stream. Any second
    // record or other name is rejected, so no unbounded linked-list walk occurs.
    blocked(
        data.NextEntryOffset == 0 && data.StreamNameLength == 14,
        "named data stream rejected",
    )?;
    let name = unsafe {
        std::slice::from_raw_parts(
            words
                .as_ptr()
                .cast::<u8>()
                .add(std::mem::offset_of!(FILE_STREAM_INFO, StreamName))
                .cast::<u16>(),
            7,
        )
    };
    blocked(
        name == [58, 58, 36, 68, 65, 84, 65],
        "named data stream rejected",
    )
}

// Holding each ancestor directory without write/delete sharing prevents rename
// and reparse conversion during a privileged walk of a live worker tree. A
// conflicting worker handle makes inspection fail closed; no unlocked retry.
fn pin_directory(path: &Path) -> Result<std::fs::File> {
    let directory = OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let metadata = directory.metadata()?;
    blocked(
        metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "reparse directory rejected",
    )?;
    Ok(directory)
}
fn walk(root: &Path, limit: u64, file_limit: usize) -> Result<Vec<PathBuf>> {
    fn visit(
        path: &Path,
        depth: usize,
        limit: u64,
        file_limit: usize,
        files: &mut Vec<PathBuf>,
        bytes: &mut u64,
    ) -> Result<()> {
        blocked(depth <= 16, "tree nesting budget exceeded")?;
        blocked(files.len() < file_limit, "tree entry budget exceeded")?;
        files.push(path.to_owned());
        let metadata = ordinary(path)?;
        reject_named_streams(path)?;
        if metadata.is_dir() {
            let _pinned = pin_directory(path)?;
            for entry in fs::read_dir(path)? {
                visit(&entry?.path(), depth + 1, limit, file_limit, files, bytes)?;
            }
        } else {
            *bytes = bytes
                .checked_add(metadata.len())
                .ok_or(Error::Blocked("tree size overflow"))?;
            blocked(*bytes <= limit, "tree disk budget exceeded")?;
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, 0, limit, file_limit, &mut files, &mut 0)?;
    Ok(files)
}
/// Test/setup utility: restrict synthetic private sentinels to owner and SYSTEM.
/// Caller must own a disposable tree. This does not change a real workspace's policy.
pub fn protect_private_tree(path: &Path) -> Result<()> {
    let owner = user_sid()?;
    for item in walk(path, 1024 * 1024 * 1024, 10000)? {
        acl(&item, &owner, None)?;
    }
    Ok(())
}

struct Profile {
    name: Vec<u16>,
    sid: PSID,
    folder: PathBuf,
    closed: bool,
    cleanup_allowed: bool,
}
impl Profile {
    fn create() -> Result<Self> {
        let name = wide(format!("EntityWorkbench.{}", uuid::Uuid::new_v4().simple()))?;
        let mut sid = null_mut();
        hr(
            unsafe {
                CreateAppContainerProfile(
                    name.as_ptr(),
                    name.as_ptr(),
                    name.as_ptr(),
                    null(),
                    0,
                    &mut sid,
                )
            },
            "CreateAppContainerProfile",
        )?;
        let mut profile = Self {
            name,
            sid,
            folder: PathBuf::new(),
            closed: false,
            cleanup_allowed: true,
        };
        let sid_text = wide(sid_string(sid)?)?;
        let mut folder = null_mut();
        hr(
            unsafe { GetAppContainerFolderPath(sid_text.as_ptr(), &mut folder) },
            "GetAppContainerFolderPath",
        )?;
        profile.folder = PathBuf::from(unsafe { from_wide(folder) });
        unsafe {
            CoTaskMemFree(folder.cast());
        }
        Ok(profile)
    }
    fn close(&mut self) -> Result<()> {
        if !self.closed {
            hr(
                unsafe { DeleteAppContainerProfile(self.name.as_ptr()) },
                "DeleteAppContainerProfile",
            )?;
            if !self.folder.as_os_str().is_empty() {
                match fs::symlink_metadata(&self.folder) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    _ => {
                        return Err(Error::Blocked(
                            "AppContainer profile storage survived cleanup",
                        ))
                    }
                }
            }
            self.closed = true;
        }
        Ok(())
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        if self.cleanup_allowed {
            let _ = self.close();
        }
        unsafe {
            FreeSid(self.sid);
        }
    }
}

struct Attributes {
    words: Vec<usize>,
    initialized: bool,
}
impl Attributes {
    fn new(count: u32) -> Result<Self> {
        let mut size = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), count, 0, &mut size);
        }
        blocked(size > 0 && size < 65536, "attribute allocation size")?;
        let mut result = Self {
            words: vec![0; size.div_ceil(size_of::<usize>())],
            initialized: false,
        };
        api(
            unsafe { InitializeProcThreadAttributeList(result.ptr(), count, 0, &mut size) },
            "InitializeAttributes",
        )?;
        result.initialized = true;
        Ok(result)
    }
    fn ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.words.as_mut_ptr().cast()
    }
    fn set<T>(&mut self, key: u32, value: &T) -> Result<()> {
        api(
            unsafe {
                UpdateProcThreadAttribute(
                    self.ptr(),
                    0,
                    key as usize,
                    (value as *const T).cast(),
                    size_of::<T>(),
                    null_mut(),
                    null(),
                )
            },
            "SetProcessAttribute",
        )
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.ptr());
            }
        }
    }
}
struct Running {
    process: Handle,
    job: Handle,
    stopped: bool,
}
impl Running {
    fn stop(&mut self) -> Result<()> {
        if !self.stopped {
            api(
                unsafe { TerminateJobObject(self.job.0, 1) },
                "TerminateWorkerJob",
            )?;
            blocked(
                unsafe { WaitForSingleObject(self.process.0, 5000) } == WAIT_OBJECT_0,
                "worker termination not acknowledged",
            )?;
            self.stopped = true;
        }
        Ok(())
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn create_job(memory: usize) -> Result<Handle> {
    let job = unsafe { CreateJobObjectW(null(), null()) };
    blocked(!job.is_null(), "job object creation failed")?;
    let job = Handle(job);
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_JOB_MEMORY
        | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
        | JOB_OBJECT_LIMIT_PROCESS_TIME;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    limits.BasicLimitInformation.PerProcessUserTimeLimit = 30 * 10_000_000;
    limits.JobMemoryLimit = memory;
    api(
        unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        },
        "SetJobLimits",
    )?;
    Ok(job)
}
fn verify_token(process: HANDLE, sid: PSID) -> Result<()> {
    let token = token(process)?;
    let app = token_info(token.0, TokenIsAppContainer)?;
    blocked(
        unsafe { *(app.as_ptr().cast::<u32>()) } == 1,
        "created process is not an AppContainer",
    )?;
    let package = token_info(token.0, TokenAppContainerSid)?;
    let package = unsafe { &*package.as_ptr().cast::<TOKEN_APPCONTAINER_INFORMATION>() };
    blocked(
        unsafe { EqualSid(package.TokenAppContainer, sid) } != 0,
        "AppContainer identity mismatch",
    )?;
    let capabilities = token_info(token.0, TokenCapabilities)?;
    blocked(
        unsafe { (*capabilities.as_ptr().cast::<TOKEN_GROUPS>()).GroupCount } == 0,
        "unexpected network/resource capabilities",
    )
}
fn clean(path: &Path, owner: &str) -> Result<()> {
    clean_entry(path, owner, 0, &mut 0)
}
fn clean_entry(path: &Path, owner: &str, depth: usize, entries: &mut usize) -> Result<()> {
    // Cleanup must remain bounded even when the worker deliberately exceeds
    // inspection limits. Stop and retain recovery state instead of stack growth.
    blocked(
        depth <= 64 && *entries < 20000,
        "cleanup tree budget exceeded",
    )?;
    *entries += 1;
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    // A junction is removed itself. Never walk/chmod a worker-created reparse target.
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        if metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
            fs::remove_dir(path)?;
        } else {
            fs::remove_file(path)?;
        }
        return Ok(());
    }
    if metadata.is_dir() {
        set_acl(path, owner, None, true)?;
        for entry in fs::read_dir(path)? {
            clean_entry(&entry?.path(), owner, depth + 1, entries)?;
        }
        fs::remove_dir(path)?;
    } else {
        // Deletion through the parent DACL does not need to change the file ACL.
        // Rust may use Windows POSIX unlink semantics that ignore READONLY for
        // this directory entry; this does not require clearing file attributes.
        fs::remove_file(path)?;
    }
    Ok(())
}
fn read_output(path: &Path) -> Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    api(
        unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) },
        "InspectOutput",
    )?;
    blocked(
        info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) == 0
            && info.nNumberOfLinks == 1,
        "invalid output file",
    )?;
    blocked(
        file.metadata()?.len() <= OUTPUT_LIMIT,
        "result exceeds bound",
    )?;
    let mut bytes = Vec::new();
    file.take(OUTPUT_LIMIT + 1).read_to_end(&mut bytes)?;
    blocked(
        bytes.len() as u64 <= OUTPUT_LIMIT,
        "result grew beyond bound",
    )?;
    Ok(bytes)
}

/// Synchronous launcher. The coordinator passes `|| token.is_cancelled()` so no
/// dependency on workbench-core or duplicate cancellation-token type is needed.
pub fn run(request: &Request, cancelled: impl Fn() -> bool) -> Result<Output> {
    validate(request)?;
    blocked(!cancelled(), "cancelled before launch")?;
    ordinary(&request.scratch_parent)?;
    let owner = user_sid()?;
    let temporary = tempfile::Builder::new()
        .prefix("ew-appcontainer-")
        .tempdir_in(&request.scratch_parent)?;
    let root = temporary.path().to_owned();
    let mut profile = Profile::create()?;
    let mut quiescent = true;
    let result = (|| {
        let sid = sid_string(profile.sid)?;
        acl(&root, &owner, Some((&sid, false)))?;
        let runtime = root.join("runtime");
        fs::create_dir(&runtime)?;
        ordinary(&request.runtime)?;
        let source = request.runtime.canonicalize()?;
        for entry in walk(&source, 1024 * 1024 * 1024, 10000)? {
            let relative = entry
                .strip_prefix(&source)
                .map_err(|_| Error::Blocked("runtime path escape"))?;
            let destination = runtime.join(relative);
            if entry == source {
                continue;
            }
            if entry.is_dir() {
                fs::create_dir_all(&destination)?;
            } else {
                fs::copy(&entry, &destination)?;
            }
        }
        for entry in walk(&runtime, 1024 * 1024 * 1024, 10000)? {
            acl(&entry, &owner, Some((&sid, false)))?;
        }
        let input = root.join("input.json");
        fs::write(&input, &request.input)?;
        acl(&input, &owner, Some((&sid, false)))?;
        let scratch = root.join("scratch");
        fs::create_dir(&scratch)?;
        acl(&scratch, &owner, Some((&sid, true)))?;
        let executable = runtime.join(&request.executable);
        blocked(
            ordinary(&executable)?.is_file(),
            "missing staged executable",
        )?;
        let path_text = |path: &Path| {
            path.to_str()
                .map(str::to_owned)
                .ok_or(Error::Blocked("invalid UTF-16 path"))
        };
        let replacements = [
            ("$EW_INPUT", path_text(&input)?),
            ("$EW_SCRATCH", path_text(&scratch)?),
            ("$EW_RUNTIME", path_text(&runtime)?),
        ];
        let mut args = vec![path_text(&executable)?];
        args.extend(request.arguments.iter().map(|arg| {
            let mut arg = arg.clone();
            for (key, value) in &replacements {
                arg = arg.replace(key, value);
            }
            arg
        }));
        let command = args
            .iter()
            .map(|arg| quote_argument(arg))
            .collect::<Vec<_>>()
            .join(" ");
        blocked(
            command.encode_utf16().count() < 30000,
            "command line exceeds bound",
        )?;
        let mut command = wide(command)?;
        let windows =
            std::env::var_os("SystemRoot").ok_or(Error::Blocked("OS SystemRoot is unavailable"))?;
        let mut environment = Vec::new();
        for (key, value) in [
            ("SystemRoot", windows.clone()),
            ("TEMP", scratch.as_os_str().to_owned()),
            ("TMP", scratch.as_os_str().to_owned()),
            ("WINDIR", windows),
        ] {
            environment.extend(wide(format!("{key}={}", value.to_string_lossy()))?);
        }
        environment.push(0);
        let capabilities = SECURITY_CAPABILITIES {
            AppContainerSid: profile.sid,
            Capabilities: null_mut(),
            CapabilityCount: 0,
            Reserved: 0,
        };
        let children: u32 = PROCESS_CREATION_CHILD_PROCESS_RESTRICTED;
        let mut attributes = Attributes::new(2)?;
        attributes.set(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, &capabilities)?;
        attributes.set(PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY, &children)?;
        let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.lpAttributeList = attributes.ptr();
        let job = create_job(request.memory_bytes)?;
        let mut process: PROCESS_INFORMATION = unsafe { zeroed() };
        let executable = wide(executable)?;
        let current_dir = wide(&scratch)?;
        // No inherited handles, including stdin/stdout/stderr. Worker IPC is files.
        api(
            unsafe {
                CreateProcessW(
                    executable.as_ptr(),
                    command.as_mut_ptr(),
                    null(),
                    null(),
                    0,
                    EXTENDED_STARTUPINFO_PRESENT
                        | CREATE_SUSPENDED
                        | CREATE_UNICODE_ENVIRONMENT
                        | CREATE_NO_WINDOW,
                    environment.as_ptr().cast(),
                    current_dir.as_ptr(),
                    &startup.StartupInfo,
                    &mut process,
                )
            },
            "CreateAppContainerProcess",
        )?;
        quiescent = false;
        let thread = Handle(process.hThread);
        let mut running = Running {
            process: Handle(process.hProcess),
            job,
            stopped: false,
        };
        // If assignment fails, terminate the still-suspended process directly: it
        // has not run untrusted code and is not yet owned by the Job Object.
        if unsafe { AssignProcessToJobObject(running.job.0, running.process.0) } == 0 {
            let failure = Error::Api {
                operation: "AssignProcessToJob",
                code: unsafe { GetLastError() },
            };
            unsafe {
                TerminateProcess(running.process.0, 1);
            }
            quiescent = unsafe { WaitForSingleObject(running.process.0, 5000) } == WAIT_OBJECT_0;
            running.stopped = quiescent;
            return Err(failure);
        }
        let operation = (|| {
            verify_token(running.process.0, profile.sid)?;
            blocked(
                unsafe { ResumeThread(thread.0) } != u32::MAX,
                "worker resume failed",
            )?;
            drop(thread);
            let start = Instant::now();
            loop {
                if cancelled() {
                    return Err(Error::Blocked("cancelled; worker job terminated"));
                }
                if start.elapsed() >= request.wall_time {
                    return Err(Error::Blocked("worker wall-time exceeded"));
                }
                walk(&scratch, WRITABLE_LIMIT, 512)?;
                walk(&profile.folder, WRITABLE_LIMIT, 512)?;
                let mut handles = 0;
                api(
                    unsafe { GetProcessHandleCount(running.process.0, &mut handles) },
                    "GetHandleCount",
                )?;
                blocked(handles <= HANDLE_LIMIT, "worker handle budget exceeded")?;
                match unsafe { WaitForSingleObject(running.process.0, 20) } {
                    WAIT_OBJECT_0 => break,
                    WAIT_TIMEOUT => {}
                    _ => return Err(Error::Blocked("worker wait failed")),
                }
            }
            walk(&scratch, WRITABLE_LIMIT, 512)?;
            walk(&profile.folder, WRITABLE_LIMIT, 512)?;
            let mut code = 0;
            api(
                unsafe { GetExitCodeProcess(running.process.0, &mut code) },
                "GetExitCode",
            )?;
            if code != 0 {
                return Err(Error::Exit(code));
            }
            read_output(&scratch.join("result.json")).map(|bytes| Output { bytes })
        })();
        if running.stop().is_err() {
            return Err(Error::Cleanup {
                prior: operation.err().map(Box::new),
            });
        }
        quiescent = true;
        operation
    })();
    let path = temporary.keep();
    if !quiescent {
        // Do not traverse/repair any worker-owned tree while a process could
        // still be running. Job Drop requests kill, but an unacknowledged exit
        // leaves the job and profile for explicit recovery; no result accepted.
        profile.cleanup_allowed = false;
        return Err(Error::Cleanup {
            prior: result.err().map(Box::new),
        });
    }
    let directory_cleanup = clean(&path, &owner);
    let profile_cleanup = profile.close();
    if directory_cleanup.is_err() || profile_cleanup.is_err() {
        return Err(Error::Cleanup {
            prior: result.err().map(Box::new),
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dacl_snapshot(path: &Path) -> Vec<u8> {
        let name = wide(path).unwrap();
        let mut descriptor = null_mut();
        let mut dacl = null_mut();
        let error = unsafe {
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            )
        };
        assert!(error == 0 && !dacl.is_null(), "synthetic DACL query failed");
        let allocation = Local(descriptor);
        let bytes =
            unsafe { std::slice::from_raw_parts(dacl.cast::<u8>(), (*dacl).AclSize as usize) }
                .to_vec();
        drop(allocation);
        bytes
    }

    #[test]
    fn file_and_directory_named_streams_are_rejected() {
        let tree = tempfile::tempdir().unwrap();
        let file = tree.path().join("small.txt");
        fs::write(&file, b"small").unwrap();
        fs::write(tree.path().join("small.txt:payload"), b"hidden").unwrap();
        assert!(matches!(
            walk(tree.path(), 1024, 20),
            Err(Error::Blocked("named data stream rejected"))
        ));
        fs::remove_file(&file).unwrap();
        let directory = tree.path().join("nested");
        fs::create_dir(&directory).unwrap();
        fs::write(tree.path().join("nested:payload"), b"hidden").unwrap();
        assert!(matches!(
            walk(tree.path(), 1024, 20),
            Err(Error::Blocked("named data stream rejected"))
        ));
    }

    #[test]
    fn overdeep_cleanup_rejects_and_retains_the_owned_tree() {
        let tree = tempfile::tempdir().unwrap();
        let scratch = tree.path().join("scratch");
        let mut nested = scratch.clone();
        fs::create_dir(&nested).unwrap();
        for _ in 0..66 {
            nested.push("x");
            fs::create_dir(&nested).unwrap();
        }
        fs::write(nested.join("retained.txt"), b"retained").unwrap();
        assert!(matches!(
            clean(&scratch, &user_sid().unwrap()),
            Err(Error::Blocked("cleanup tree budget exceeded"))
        ));
        assert_eq!(fs::read(nested.join("retained.txt")).unwrap(), b"retained");
    }

    #[test]
    fn output_rejects_hardlinks_and_large_files() {
        let tree = tempfile::tempdir().unwrap();
        let output = tree.path().join("result.json");
        let alias = tree.path().join("alias.json");
        fs::write(&output, b"{}").unwrap();
        fs::hard_link(&output, &alias).unwrap();
        assert!(matches!(
            read_output(&output),
            Err(Error::Blocked("invalid output file"))
        ));
        fs::remove_file(alias).unwrap();
        fs::write(&output, vec![0; OUTPUT_LIMIT as usize + 1]).unwrap();
        assert!(matches!(
            read_output(&output),
            Err(Error::Blocked("result exceeds bound"))
        ));
    }

    #[test]
    fn cleanup_preserves_outside_readonly_hardlink_state() {
        let tree = tempfile::tempdir().unwrap();
        let outside = tree.path().join("outside.txt");
        let scratch = tree.path().join("scratch");
        fs::create_dir(&scratch).unwrap();
        fs::write(&outside, b"retained").unwrap();
        fs::hard_link(&outside, scratch.join("alias.txt")).unwrap();
        let original_permissions = fs::metadata(&outside).unwrap().permissions();
        let mut permissions = original_permissions.clone();
        permissions.set_readonly(true);
        fs::set_permissions(&outside, permissions).unwrap();
        let dacl_before = dacl_snapshot(&outside);
        let attributes_before = fs::metadata(&outside).unwrap().file_attributes();
        let outcome = clean(&scratch, &user_sid().unwrap());
        assert!(
            dacl_before == dacl_snapshot(&outside),
            "cleanup changed outside hardlink DACL"
        );
        assert_eq!(
            fs::metadata(&outside).unwrap().file_attributes(),
            attributes_before,
            "cleanup changed outside hardlink attributes"
        );
        assert_eq!(fs::read(&outside).unwrap(), b"retained");
        // Modern Windows permits POSIX unlink while ignoring READONLY. Other
        // filesystems may refuse deletion. Neither outcome may mutate the
        // surviving outside hardlink's content, attributes or DACL.
        match outcome {
            Ok(()) => assert!(!scratch.exists(), "successful cleanup retained scratch"),
            Err(Error::Io(std::io::ErrorKind::PermissionDenied)) => {
                assert!(
                    scratch.join("alias.txt").exists(),
                    "denied cleanup lost retained alias"
                );
            }
            Err(error) => panic!("unexpected cleanup error: {error}"),
        }
        // Restore only this test-owned sentinel after checking the boundary.
        fs::set_permissions(&outside, original_permissions).unwrap();
        clean(&scratch, &user_sid().unwrap()).unwrap();
    }

    #[test]
    fn cleanup_reports_delete_sharing_failure_until_handle_closes() {
        let tree = tempfile::tempdir().unwrap();
        let scratch = tree.path().join("scratch");
        fs::create_dir(&scratch).unwrap();
        let file = scratch.join("locked.txt");
        fs::write(&file, b"retained").unwrap();
        // Denying delete sharing is a real kernel obstacle independent of the
        // read-only attribute and POSIX unlink support.
        let handle = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&file)
            .unwrap();
        assert!(
            matches!(clean(&scratch, &user_sid().unwrap()), Err(Error::Io(_))),
            "cleanup hid a denied unlink or failed for an unrelated reason"
        );
        assert_eq!(fs::read(&file).unwrap(), b"retained");
        drop(handle);
        clean(&scratch, &user_sid().unwrap()).unwrap();
        assert!(
            !scratch.exists(),
            "cleanup failed after the denying handle closed"
        );
    }

    #[test]
    fn junction_is_rejected_and_cleanup_preserves_target() {
        let tree = tempfile::tempdir().unwrap();
        let outside = tree.path().join("outside");
        let scratch = tree.path().join("scratch");
        fs::create_dir(&outside).unwrap();
        fs::create_dir(&scratch).unwrap();
        fs::write(outside.join("retained.txt"), b"retained").unwrap();
        let link = scratch.join("junction");
        // A junction needs no symbolic-link privilege. This fixed test command
        // operates only on tempfile paths, never user input or production jobs.
        let status = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(status.status.success(), "synthetic junction setup failed");
        assert!(matches!(
            walk(&scratch, 1024, 20),
            Err(Error::Blocked("reparse path rejected"))
        ));
        clean(&scratch, &user_sid().unwrap()).unwrap();
        assert_eq!(fs::read(outside.join("retained.txt")).unwrap(), b"retained");
    }

    #[test]
    fn pinned_directory_prevents_rename_during_inspection() {
        let tree = tempfile::tempdir().unwrap();
        let directory = tree.path().join("pinned");
        fs::create_dir(&directory).unwrap();
        let pinned = pin_directory(&directory).unwrap();
        assert!(fs::rename(&directory, tree.path().join("moved")).is_err());
        drop(pinned);
        fs::rename(&directory, tree.path().join("moved")).unwrap();
    }
}
