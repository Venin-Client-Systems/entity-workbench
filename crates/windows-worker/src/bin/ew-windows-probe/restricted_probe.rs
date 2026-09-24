//! Synthetic worker-owned directory lockout. No launcher permissions are changed.
use std::{
    ffi::{c_void, OsStr},
    fs::{File, OpenOptions},
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};
use workbench_windows_worker::{Error, ProbeCheckpoint, Result};
type AnyResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Descriptor(*mut c_void);
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}
fn native(result: i32, operation: &'static str) -> Result<()> {
    if result == 0 {
        Err(Error::Api {
            operation,
            code: unsafe { GetLastError() },
        })
    } else {
        Ok(())
    }
}
fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain([0]).collect()
}
fn descriptor(sddl: &str) -> Result<Descriptor> {
    let text = wide(OsStr::new(sddl));
    let mut value = null_mut();
    native(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                1,
                &mut value,
                null_mut(),
            )
        },
        "RestrictedDescriptor",
    )?;
    Ok(Descriptor(value))
}
fn token_info(token: HANDLE, class: TOKEN_INFORMATION_CLASS, minimum: usize) -> Result<Vec<usize>> {
    let mut size = 0;
    unsafe { GetTokenInformation(token, class, null_mut(), 0, &mut size) };
    if !(minimum..=4096).contains(&(size as usize)) {
        return Err(Error::Blocked("invalid synthetic token data size"));
    }
    let capacity = size;
    let mut value = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    native(
        unsafe {
            GetTokenInformation(token, class, value.as_mut_ptr().cast(), capacity, &mut size)
        },
        "RestrictedTokenInformation",
    )?;
    if size < minimum as u32 || size > capacity {
        return Err(Error::Blocked("invalid synthetic token result size"));
    }
    Ok(value)
}
fn sid_text(sid: PSID) -> Result<String> {
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
        return Err(Error::Blocked("invalid synthetic SID"));
    }
    let mut value = null_mut();
    native(
        unsafe { ConvertSidToStringSidW(sid, &mut value) },
        "RestrictedSidString",
    )?;
    let allocation = Descriptor(value.cast());
    // This allocation is returned by Windows, not by the worker's input. A SID
    // string has a bounded OS representation; no SID is logged or serialized.
    let length = (0..256)
        .find(|&index| unsafe { *value.add(index) } == 0)
        .ok_or(Error::Blocked("synthetic SID string exceeds bound"))?;
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(value, length) })
        .map_err(|_| Error::Blocked("invalid synthetic SID encoding"));
    drop(allocation);
    result
}
fn creation_descriptor() -> Result<Descriptor> {
    let mut raw = null_mut();
    native(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) },
        "RestrictedTokenOpen",
    )?;
    // Successful OpenProcessToken transfers one owned, non-inherited handle.
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    let user = token_info(token.as_raw_handle(), TokenUser, size_of::<TOKEN_USER>())?;
    let package = token_info(
        token.as_raw_handle(),
        TokenAppContainerSid,
        size_of::<TOKEN_APPCONTAINER_INFORMATION>(),
    )?;
    let user_sid = sid_text(unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid })?;
    let package_sid =
        unsafe { (*package.as_ptr().cast::<TOKEN_APPCONTAINER_INFORMATION>()).TokenAppContainer };
    let package_ace = if package_sid.is_null() {
        String::new()
    } else {
        format!("(A;;FA;;;{})", sid_text(package_sid)?)
    };
    // Both sides of AppContainer's dual-principal check receive access only to
    // this new empty child. The assigned parent/input/runtime ACLs are untouched.
    descriptor(&format!("D:P(A;;FA;;;{user_sid}){package_ace}"))
}
fn assign_dacl(file: &File, descriptor: &Descriptor) -> Result<()> {
    let (mut present, mut defaulted) = (0, 0);
    let mut dacl = null_mut();
    native(
        unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) },
        "RestrictedGetDacl",
    )?;
    if present == 0 || dacl.is_null() {
        return Err(Error::Blocked("synthetic null DACL forbidden"));
    }
    let code = unsafe {
        SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            dacl,
            null(),
        )
    };
    if code != 0 {
        return Err(Error::Api {
            operation: "RestrictedSetDacl",
            code,
        });
    }
    Ok(())
}
fn verify_empty(file: &File) -> Result<()> {
    let mut dacl = null_mut();
    let mut raw = null_mut();
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut raw,
        )
    };
    if code != 0 {
        return Err(Error::Api {
            operation: "RestrictedInspectDacl",
            code,
        });
    }
    let descriptor = Descriptor(raw);
    let (mut control, mut revision) = (0, 0);
    native(
        unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) },
        "RestrictedDaclControl",
    )?;
    if dacl.is_null()
        || unsafe { IsValidAcl(dacl) } == 0
        || unsafe { (*dacl).AceCount } != 0
        || control & SE_DACL_PROTECTED == 0
        || control & SE_DACL_PRESENT == 0
    {
        return Err(Error::Blocked("synthetic DACL is not protected and empty"));
    }
    Ok(())
}
fn checked<T>(
    result: Result<T>,
    failed: fn(u32) -> ProbeCheckpoint,
    checkpoint: &impl Fn(ProbeCheckpoint) -> AnyResult<()>,
) -> AnyResult<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let code = match &error {
                Error::Api { code, .. } => *code,
                Error::Io(kind) if *kind == std::io::ErrorKind::PermissionDenied => {
                    ERROR_ACCESS_DENIED
                }
                _ => ERROR_INVALID_DATA,
            };
            checkpoint(failed(code))?;
            Err(error.into())
        }
    }
}
/// The returned handle retains security/attribute rights for fixture inspection
/// and unit-test restoration. The confined caller drops it before returning.
/// All native pointer arguments borrow live, aligned OS allocations. A failed
/// CreateDirectory never opens or changes an existing path.
pub fn create(
    path: &Path,
    checkpoint: impl Fn(ProbeCheckpoint) -> AnyResult<()>,
) -> AnyResult<File> {
    let creation = checked(
        creation_descriptor(),
        |code| ProbeCheckpoint::RestrictedIdentityFailed { code },
        &checkpoint,
    )?;
    let empty = checked(
        descriptor("D:P"),
        |code| ProbeCheckpoint::RestrictedDescriptorFailed { code },
        &checkpoint,
    )?;
    checkpoint(ProbeCheckpoint::RestrictedDescriptorReady)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: creation.0,
        bInheritHandle: 0,
    };
    let name = wide(path.as_os_str());
    checkpoint(ProbeCheckpoint::RestrictedDirectoryCreate)?;
    checked(
        native(
            unsafe { CreateDirectoryW(name.as_ptr(), &attributes) },
            "CreateRestrictedDirectory",
        ),
        |code| ProbeCheckpoint::RestrictedDirectoryCreateFailed { code },
        &checkpoint,
    )?;
    checkpoint(ProbeCheckpoint::RestrictedDirectoryCreated)?;
    let directory = checked(
        OpenOptions::new()
            .access_mode(READ_CONTROL | WRITE_DAC | FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
            )
            .open(path)
            .map_err(|error| Error::Api {
                operation: "RestrictedOpenDirectory",
                code: error
                    .raw_os_error()
                    .map_or(ERROR_GEN_FAILURE, |code| code as u32),
            }),
        |code| ProbeCheckpoint::RestrictedOpenFailed { code },
        &checkpoint,
    )?;
    let metadata = directory.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(Error::Blocked("invalid synthetic directory handle").into());
    }
    checkpoint(ProbeCheckpoint::RestrictedDaclSet)?;
    checked(
        assign_dacl(&directory, &empty),
        |code| ProbeCheckpoint::RestrictedDaclSetFailed { code },
        &checkpoint,
    )?;
    checked(
        verify_empty(&directory),
        |code| ProbeCheckpoint::RestrictedDaclVerifyFailed { code },
        &checkpoint,
    )?;
    checkpoint(ProbeCheckpoint::RestrictedDaclVerified)?;
    Ok(directory)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreationAttempt {
    created: bool,
    create_code: u32,
    roundtrip_code: Option<u32>,
}
impl CreationAttempt {
    fn passed(&self) -> bool {
        self.created && self.create_code == 0 && self.roundtrip_code == Some(0)
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryControls {
    parent_add_opened: bool,
    parent_add_code: u32,
    ordinary_relative: CreationAttempt,
    ordinary_absolute: CreationAttempt,
    explicit_relative: CreationAttempt,
    explicit_absolute: CreationAttempt,
}
impl DirectoryControls {
    pub fn passed(&self) -> bool {
        self.parent_add_opened
            && self.parent_add_code == 0
            && [
                &self.ordinary_relative,
                &self.ordinary_absolute,
                &self.explicit_relative,
                &self.explicit_absolute,
            ]
            .iter()
            .all(|attempt| attempt.passed())
    }
}
fn io_code(error: &std::io::Error) -> u32 {
    error
        .raw_os_error()
        .map_or(ERROR_GEN_FAILURE, |code| code as u32)
}
fn create_attempt(path: &Path, descriptor: Option<&Descriptor>) -> CreationAttempt {
    let name = wide(path.as_os_str());
    let attributes = descriptor.map(|value| SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: value.0,
        bInheritHandle: 0,
    });
    let result = unsafe {
        CreateDirectoryW(
            name.as_ptr(),
            attributes.as_ref().map_or(null(), |value| value),
        )
    };
    if result == 0 {
        // Capture the native result before any further API or diagnostic call.
        return CreationAttempt {
            created: false,
            create_code: unsafe { GetLastError() },
            roundtrip_code: None,
        };
    }
    let payload = path.join("sentinel.txt");
    let roundtrip = std::fs::write(&payload, b"synthetic child directory")
        .and_then(|()| std::fs::read(&payload));
    let code = match roundtrip {
        Ok(value) if value == b"synthetic child directory" => 0,
        Ok(_) => ERROR_INVALID_DATA,
        Err(error) => io_code(&error),
    };
    CreationAttempt {
        created: true,
        create_code: 0,
        roundtrip_code: Some(code),
    }
}
fn directory_controls_at(relative_root: &Path, absolute_root: &Path) -> Result<DirectoryControls> {
    let descriptor = creation_descriptor()?;
    let parent = OpenOptions::new()
        .access_mode(FILE_ADD_SUBDIRECTORY)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
        )
        .open(absolute_root);
    let (parent_add_opened, parent_add_code) = match parent {
        Ok(handle) => {
            drop(handle);
            (true, 0)
        }
        Err(error) => (false, io_code(&error)),
    };
    Ok(DirectoryControls {
        parent_add_opened,
        parent_add_code,
        ordinary_relative: create_attempt(&relative_root.join("ordinary-relative"), None),
        ordinary_absolute: create_attempt(&absolute_root.join("ordinary-absolute"), None),
        explicit_relative: create_attempt(
            &relative_root.join("explicit-relative"),
            Some(&descriptor),
        ),
        explicit_absolute: create_attempt(
            &absolute_root.join("explicit-absolute"),
            Some(&descriptor),
        ),
    })
}
/// Diagnostic comparison only: the same paths and calls run both unconfined and
/// confined. The report contains no path, SID or descriptor. No ACL on the parent
/// is changed, and there are no retries or alternative production launch modes.
pub fn directory_controls() -> Result<DirectoryControls> {
    directory_controls_at(Path::new("."), &std::env::current_dir()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_creation_controls_require_creation_and_roundtrip_success() {
        let tree = tempfile::tempdir().unwrap();
        let controls = directory_controls_at(tree.path(), tree.path()).unwrap();
        assert!(
            controls.passed(),
            "unconfined directory creation controls failed: {controls:?}"
        );
        let mut failed = controls.clone();
        failed.parent_add_opened = false;
        assert!(!failed.passed());
        let mut failed = controls.clone();
        failed.explicit_relative.created = false;
        assert!(!failed.passed());
        let mut failed = controls.clone();
        failed.ordinary_absolute.roundtrip_code = None;
        assert!(!failed.passed());
        let mut failed = controls;
        failed.explicit_absolute.create_code = ERROR_ACCESS_DENIED;
        assert!(!failed.passed());
    }
    #[test]
    fn created_child_locks_out_new_inspection_but_retains_security_handle() {
        let tree = tempfile::tempdir().unwrap();
        let path = tree.path().join("restricted");
        let file = create(&path, |_| Ok(())).unwrap();
        verify_empty(&file).unwrap();
        assert_eq!(
            std::fs::read_dir(&path).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        // Restore only this owned empty fixture through its pre-lockout handle.
        assign_dacl(&file, &creation_descriptor().unwrap()).unwrap();
        drop(file);
        std::fs::remove_dir(&path).unwrap();
    }
    #[test]
    fn fixture_refuses_existing_directory_without_locking_it() {
        let tree = tempfile::tempdir().unwrap();
        std::fs::write(tree.path().join("sentinel"), b"unchanged").unwrap();
        assert!(create(tree.path(), |_| Ok(())).is_err());
        assert_eq!(
            std::fs::read(tree.path().join("sentinel")).unwrap(),
            b"unchanged"
        );
        assert!(std::fs::read_dir(tree.path()).is_ok());
    }
}
