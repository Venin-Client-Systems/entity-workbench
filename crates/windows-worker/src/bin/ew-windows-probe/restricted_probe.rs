//! Synthetic worker-owned directory lockout. No launcher permissions are changed.
use super::rewrite_outcome::{RewriteObservation, RewriteOutcome};
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
    System::{
        SystemServices::{
            ACCESS_ALLOWED_ACE_TYPE, SYSTEM_MANDATORY_LABEL_ACE_TYPE,
            SYSTEM_MANDATORY_LABEL_NO_WRITE_UP,
        },
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};
use workbench_windows_worker::{Error, Result};

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
fn creation_descriptor(low_label: bool) -> Result<Descriptor> {
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
    let label = if low_label { "S:(ML;;NW;;;LW)" } else { "" };
    descriptor(&format!("D:P(A;;FA;;;{user_sid}){package_ace}{label}"))
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
fn descriptor_has_low_label(descriptor: &Descriptor) -> Result<bool> {
    let (mut present, mut defaulted) = (0, 0);
    let mut sacl = null_mut();
    native(
        unsafe { GetSecurityDescriptorSacl(descriptor.0, &mut present, &mut sacl, &mut defaulted) },
        "RestrictedLabelSacl",
    )?;
    if present == 0 || sacl.is_null() {
        return Ok(false);
    }
    if unsafe { IsValidAcl(sacl) } == 0 {
        return Err(Error::Blocked("invalid synthetic label ACL"));
    }
    if unsafe { (*sacl).AceCount } != 1 {
        return Ok(false);
    }
    let mut raw = null_mut();
    native(unsafe { GetAce(sacl, 0, &mut raw) }, "RestrictedLabelAce")?;
    let ace = raw.cast::<SYSTEM_MANDATORY_LABEL_ACE>();
    // IsValidAcl/GetAce establish the enclosing ACE bounds. Validate its label
    // type and fixed fields before reading the trailing, OS-returned SID.
    if unsafe { (*ace).Header.AceSize }
        < (std::mem::offset_of!(SYSTEM_MANDATORY_LABEL_ACE, SidStart) + 12) as u16
        || unsafe { (*ace).Header.AceType } as u32 != SYSTEM_MANDATORY_LABEL_ACE_TYPE
        || (unsafe { (*ace).Header.AceFlags } as u32) & INHERIT_ONLY_ACE != 0
        || unsafe { (*ace).Mask } != SYSTEM_MANDATORY_LABEL_NO_WRITE_UP
    {
        return Ok(false);
    }
    let sid: PSID = unsafe { std::ptr::addr_of_mut!((*ace).SidStart) }.cast();
    // A low integrity SID has one subauthority (12 bytes). The earlier ACE
    // length check covers that entire representation before SID APIs inspect it.
    if unsafe { *sid.cast::<u8>().add(1) } != 1 {
        return Ok(false);
    }
    Ok(unsafe { IsValidSid(sid) } != 0 && unsafe { IsWellKnownSid(sid, WinLowLabelSid) } != 0)
}
fn read_low_label(file: &File) -> Result<bool> {
    let mut raw = null_mut();
    // LABEL_SECURITY_INFORMATION requires READ_CONTROL, not audit-SACL access
    // or a security privilege. Request only the mandatory label descriptor.
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            LABEL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut raw,
        )
    };
    if code != 0 {
        return Err(Error::Api {
            operation: "RestrictedReadLabel",
            code,
        });
    }
    descriptor_has_low_label(&Descriptor(raw))
}
fn capture_dacl(file: &File) -> Result<Descriptor> {
    let mut raw = null_mut();
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut raw,
        )
    };
    if code != 0 {
        return Err(Error::Api {
            operation: "RewriteCaptureDacl",
            code,
        });
    }
    Ok(Descriptor(raw))
}
fn dacl_signature(descriptor: &Descriptor) -> Result<(bool, Vec<Vec<u8>>)> {
    let (mut present, mut defaulted, mut control, mut revision) = (0, 0, 0, 0);
    let mut dacl = null_mut();
    native(
        unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) },
        "RewriteDacl",
    )?;
    native(
        unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) },
        "RewriteDaclControl",
    )?;
    if present == 0
        || dacl.is_null()
        || unsafe { IsValidAcl(dacl) } == 0
        || unsafe { (*dacl).AceCount } > 32
    {
        return Err(Error::Blocked("invalid bounded rewrite DACL"));
    }
    let mut entries = Vec::new();
    for index in 0..unsafe { (*dacl).AceCount } as u32 {
        let mut raw = null_mut();
        native(unsafe { GetAce(dacl, index, &mut raw) }, "RewriteDaclAce")?;
        let length = unsafe { (*raw.cast::<ACE_HEADER>()).AceSize } as usize;
        entries.push(unsafe { std::slice::from_raw_parts(raw.cast::<u8>(), length) }.to_vec());
    }
    Ok((control & SE_DACL_PROTECTED != 0, entries))
}
fn restore_dacl(file: &File, original: &Descriptor) -> Result<()> {
    let (mut present, mut defaulted, mut control, mut revision) = (0, 0, 0, 0);
    let mut dacl = null_mut();
    native(
        unsafe { GetSecurityDescriptorDacl(original.0, &mut present, &mut dacl, &mut defaulted) },
        "RewriteRestoreDacl",
    )?;
    native(
        unsafe { GetSecurityDescriptorControl(original.0, &mut control, &mut revision) },
        "RewriteRestoreControl",
    )?;
    if present == 0 || dacl.is_null() {
        return Err(Error::Blocked("rewrite null restore DACL forbidden"));
    }
    let protection = if control & SE_DACL_PROTECTED != 0 {
        PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        UNPROTECTED_DACL_SECURITY_INFORMATION
    };
    let code = unsafe {
        SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | protection,
            null_mut(),
            null_mut(),
            dacl,
            null(),
        )
    };
    if code != 0 {
        return Err(Error::Api {
            operation: "RewriteRestore",
            code,
        });
    }
    Ok(())
}
/// Same hostile attempt for baseline and AppContainer. The baseline must really
/// apply an empty DACL and restore through its held security handle. A confined
/// denial has an exact API context/code; no generic process error is converted.
pub fn rewrite_attempt(path: &Path) -> Result<RewriteObservation> {
    let name = wide(path.as_os_str());
    native(
        unsafe { CreateDirectoryW(name.as_ptr(), null()) },
        "RewriteCreateDirectory",
    )?;
    let reader = OpenOptions::new()
        .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
        )
        .open(path)?;
    let metadata = reader.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(Error::Blocked(
            "rewrite target is not an ordinary owned directory",
        ));
    }
    let before = capture_dacl(&reader)?;
    let signature = dacl_signature(&before)?;
    let low_label = read_low_label(&reader)?;
    let outcome = match open_dacl_handle(path) {
        Err(Error::Api {
            operation: "RestrictedOpenDirectory",
            code: ERROR_ACCESS_DENIED,
        }) => RewriteOutcome::WriteDacOpenDenied {
            code: ERROR_ACCESS_DENIED,
        },
        Err(error) => return Err(error),
        Ok(writer) => {
            let exercise = (|| -> Result<()> {
                assign_dacl(&writer, &descriptor("D:P")?)?;
                verify_empty(&writer)?;
                if !matches!(std::fs::read_dir(path), Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied)
                {
                    return Err(Error::Blocked(
                        "empty-DACL positive control remained inspectable",
                    ));
                }
                Ok(())
            })();
            // Always attempt restoration, even if verification failed. No result
            // qualifies if either operation or restoration failed.
            restore_dacl(&writer, &before)?;
            exercise?;
            RewriteOutcome::AppliedAndRestored {
                empty_dacl_verified: true,
                inspection_denied: true,
            }
        }
    };
    let dacl_unchanged = dacl_signature(&capture_dacl(&reader)?)? == signature;
    let directory_readable = match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => false,
    };
    let payload = path.join("rewrite-sentinel.txt");
    let roundtrip = std::fs::write(&payload, b"synthetic rewrite probe")
        .and_then(|()| std::fs::read(&payload))
        .is_ok_and(|bytes| bytes == b"synthetic rewrite probe");
    Ok(RewriteObservation {
        outcome,
        low_label,
        dacl_unchanged,
        directory_readable,
        roundtrip,
    })
}

fn open_dacl_handle(path: &Path) -> Result<File> {
    OpenOptions::new()
        .access_mode(READ_CONTROL | WRITE_DAC | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
        )
        .open(path)
        .map_err(|error| Error::Api {
            operation: "RestrictedOpenDirectory",
            code: io_code(&error),
        })
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ChildAcl {
    all_simple_allow_aces: bool,
    owner_is_user: bool,
    owner_is_default: bool,
    owner_is_package: bool,
    owner_rights_present: bool,
    owner_rights_inherited: bool,
    owner_rights_write_dac: bool,
    package_present: bool,
    package_inherited: bool,
    package_write_dac: bool,
    user_write_dac: bool,
}
fn child_acl(file: &File) -> Result<ChildAcl> {
    let mut token = null_mut();
    native(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) },
        "ChildAclToken",
    )?;
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let user = token_info(token.as_raw_handle(), TokenUser, size_of::<TOKEN_USER>())?;
    let default = token_info(token.as_raw_handle(), TokenOwner, size_of::<TOKEN_OWNER>())?;
    let package = token_info(
        token.as_raw_handle(),
        TokenAppContainerSid,
        size_of::<TOKEN_APPCONTAINER_INFORMATION>(),
    )?;
    let user_sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let default_sid = unsafe { (*default.as_ptr().cast::<TOKEN_OWNER>()).Owner };
    let package_sid =
        unsafe { (*package.as_ptr().cast::<TOKEN_APPCONTAINER_INFORMATION>()).TokenAppContainer };
    let (mut owner, mut raw, mut dacl) = (null_mut(), null_mut(), null_mut());
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut raw,
        )
    };
    if code != 0 {
        return Err(Error::Api {
            operation: "ChildAclQuery",
            code,
        });
    }
    let _descriptor = Descriptor(raw);
    if owner.is_null()
        || dacl.is_null()
        || unsafe { IsValidAcl(dacl) } == 0
        || unsafe { (*dacl).AceCount } > 32
    {
        return Err(Error::Blocked("invalid synthetic child ACL"));
    }
    let mut summary = ChildAcl {
        all_simple_allow_aces: true,
        owner_is_user: unsafe { EqualSid(owner, user_sid) } != 0,
        owner_is_default: unsafe { EqualSid(owner, default_sid) } != 0,
        owner_is_package: !package_sid.is_null() && unsafe { EqualSid(owner, package_sid) } != 0,
        ..Default::default()
    };
    for index in 0..unsafe { (*dacl).AceCount } as u32 {
        let mut raw = null_mut();
        native(unsafe { GetAce(dacl, index, &mut raw) }, "ChildAclAce")?;
        let ace = raw.cast::<ACCESS_ALLOWED_ACE>();
        if unsafe { (*ace).Header.AceType } as u32 != ACCESS_ALLOWED_ACE_TYPE {
            summary.all_simple_allow_aces = false;
            continue;
        }
        let prefix = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
        let size = unsafe { (*ace).Header.AceSize } as usize;
        if size < prefix + 8 {
            return Err(Error::Blocked("short synthetic child ACE"));
        }
        let sid: PSID = unsafe { std::ptr::addr_of_mut!((*ace).SidStart) }.cast();
        let subauthorities = unsafe { *sid.cast::<u8>().add(1) } as usize;
        if subauthorities > 15
            || prefix + 8 + 4 * subauthorities > size
            || unsafe { IsValidSid(sid) } == 0
        {
            return Err(Error::Blocked("invalid synthetic child ACE SID"));
        }
        let flags = unsafe { (*ace).Header.AceFlags } as u32;
        if flags & INHERIT_ONLY_ACE != 0 {
            continue;
        }
        let inherited = flags & INHERITED_ACE != 0;
        let write_dac = unsafe { (*ace).Mask } & (WRITE_DAC | GENERIC_ALL) != 0;
        if unsafe { IsWellKnownSid(sid, WinCreatorOwnerRightsSid) } != 0 {
            summary.owner_rights_present = true;
            summary.owner_rights_inherited |= inherited;
            summary.owner_rights_write_dac |= write_dac;
        }
        if !package_sid.is_null() && unsafe { EqualSid(sid, package_sid) } != 0 {
            summary.package_present = true;
            summary.package_inherited |= inherited;
            summary.package_write_dac |= write_dac;
        }
        if unsafe { EqualSid(sid, user_sid) } != 0 {
            summary.user_write_dac |= write_dac;
        }
    }
    Ok(summary)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreationAttempt {
    created: bool,
    create_code: u32,
    roundtrip_code: Option<u32>,
    low_label: Option<bool>,
    label_code: Option<u32>,
    write_dac_opened: Option<bool>,
    write_dac_code: Option<u32>,
    acl: Option<ChildAcl>,
    acl_code: Option<u32>,
}
impl CreationAttempt {
    fn passed(&self) -> bool {
        self.created && self.create_code == 0 && self.roundtrip_code == Some(0)
    }
    fn passed_labelled(&self) -> bool {
        self.passed() && self.low_label == Some(true) && self.label_code == Some(0)
    }
    fn observed_explicit(&self) -> bool {
        self.passed()
            || (!self.created
                && self.create_code == ERROR_ACCESS_DENIED
                && self.roundtrip_code.is_none()
                && self.low_label.is_none()
                && self.label_code.is_none()
                && self.write_dac_opened.is_none()
                && self.write_dac_code.is_none()
                && self.acl.is_none()
                && self.acl_code.is_none())
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
    labelled_relative: CreationAttempt,
    labelled_absolute: CreationAttempt,
}
impl DirectoryControls {
    pub fn baseline_passed(&self) -> bool {
        self.parent_add_opened
            && self.parent_add_code == 0
            && self.ordinary_relative.passed()
            && self.ordinary_absolute.passed()
            && self.ordinary_relative.write_dac_opened == Some(true)
            && self.ordinary_relative.write_dac_code == Some(0)
            && self.ordinary_absolute.write_dac_opened == Some(true)
            && self.ordinary_absolute.write_dac_code == Some(0)
            && self.explicit_relative.passed()
            && self.explicit_absolute.passed()
            && self.labelled_relative.passed_labelled()
            && self.labelled_absolute.passed_labelled()
    }
    pub fn passed(&self) -> bool {
        self.parent_add_opened
            && self.parent_add_code == 0
            && self.ordinary_relative.passed_labelled()
            && self.ordinary_absolute.passed_labelled()
            && self.explicit_relative.observed_explicit()
            && self.explicit_absolute.observed_explicit()
            && self.labelled_relative.observed_explicit()
            && self.labelled_absolute.observed_explicit()
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
            low_label: None,
            label_code: None,
            write_dac_opened: None,
            write_dac_code: None,
            acl: None,
            acl_code: None,
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
    let inspection = OpenOptions::new()
        .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
        )
        .open(path)
        .map_err(|error| Error::Api {
            operation: "RestrictedControlLabelOpen",
            code: io_code(&error),
        })
        .and_then(|file| read_low_label(&file));
    let (low_label, label_code) = match inspection {
        Ok(value) => (Some(value), Some(0)),
        Err(Error::Api { code, .. }) => (None, Some(code)),
        Err(_) => (None, Some(ERROR_INVALID_DATA)),
    };
    let security = open_dacl_handle(path);
    let (write_dac_opened, write_dac_code) = match security {
        Ok(file) => {
            drop(file);
            (Some(true), Some(0))
        }
        Err(Error::Api { code, .. }) => (Some(false), Some(code)),
        Err(_) => (Some(false), Some(ERROR_INVALID_DATA)),
    };
    let acl = OpenOptions::new()
        .access_mode(READ_CONTROL)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
        )
        .open(path)
        .map_err(|error| Error::Api {
            operation: "ChildAclOpen",
            code: io_code(&error),
        })
        .and_then(|file| child_acl(&file));
    let (acl, acl_code) = match acl {
        Ok(value) => (Some(value), Some(0)),
        Err(Error::Api { code, .. }) => (None, Some(code)),
        Err(_) => (None, Some(ERROR_INVALID_DATA)),
    };
    CreationAttempt {
        created: true,
        create_code: 0,
        roundtrip_code: Some(code),
        low_label,
        label_code,
        write_dac_opened,
        write_dac_code,
        acl,
        acl_code,
    }
}
fn directory_controls_at(relative_root: &Path, absolute_root: &Path) -> Result<DirectoryControls> {
    let descriptor = creation_descriptor(false)?;
    let labelled = creation_descriptor(true)?;
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
        labelled_relative: create_attempt(
            &relative_root.join("labelled-relative"),
            Some(&labelled),
        ),
        labelled_absolute: create_attempt(
            &absolute_root.join("labelled-absolute"),
            Some(&labelled),
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
            controls.baseline_passed(),
            "unconfined directory creation controls failed: {controls:?}"
        );
        assert!(controls.ordinary_relative.acl.is_some());
        assert_eq!(controls.ordinary_relative.acl_code, Some(0));
        assert!(controls.ordinary_absolute.acl.is_some());
        assert_eq!(controls.ordinary_absolute.acl_code, Some(0));
        let mut controls = controls;
        controls.ordinary_relative.low_label = Some(true);
        controls.ordinary_absolute.low_label = Some(true);
        assert!(controls.passed());
        let mut failed = controls.clone();
        failed.parent_add_opened = false;
        assert!(!failed.passed());
        let mut failed = controls.clone();
        failed.ordinary_relative.created = false;
        assert!(!failed.passed());
        let mut failed = controls.clone();
        failed.ordinary_absolute.roundtrip_code = None;
        assert!(!failed.passed());
        let mut failed = controls.clone();
        failed.ordinary_absolute.low_label = Some(false);
        assert!(!failed.passed());
        let mut failed = controls.clone();
        failed.ordinary_absolute.label_code = Some(ERROR_ACCESS_DENIED);
        assert!(!failed.passed());
        let mut denied = controls;
        denied.explicit_relative = CreationAttempt {
            created: false,
            create_code: ERROR_ACCESS_DENIED,
            roundtrip_code: None,
            low_label: None,
            label_code: None,
            write_dac_opened: None,
            write_dac_code: None,
            acl: None,
            acl_code: None,
        };
        denied.labelled_relative = denied.explicit_relative.clone();
        assert!(denied.passed());
        assert!(!denied.baseline_passed());
        denied.explicit_relative.create_code = ERROR_ALREADY_EXISTS;
        assert!(!denied.passed());
    }
    #[test]
    fn unconfined_rewrite_control_applies_denies_inspection_and_restores() {
        let tree = tempfile::tempdir().unwrap();
        let observation = rewrite_attempt(&tree.path().join("rewrite")).unwrap();
        assert!(
            observation.positive_control(),
            "typed rewrite control failed: {observation:?}"
        );
    }
    #[test]
    fn fixture_descriptor_changes_only_the_mandatory_label() {
        let unlabelled = creation_descriptor(false).unwrap();
        let labelled = creation_descriptor(true).unwrap();
        assert!(!descriptor_has_low_label(&unlabelled).unwrap());
        assert!(descriptor_has_low_label(&labelled).unwrap());
        for sddl in ["S:(ML;;NW;;;ME)", "S:(ML;;NR;;;LW)", "S:(ML;IO;NW;;;LW)"] {
            assert!(!descriptor_has_low_label(&descriptor(sddl).unwrap()).unwrap());
        }
        fn acl_bytes(descriptor: &Descriptor) -> Vec<u8> {
            let (mut present, mut defaulted) = (0, 0);
            let mut dacl = null_mut();
            native(
                unsafe {
                    GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted)
                },
                "SyntheticDacl",
            )
            .unwrap();
            assert!(present != 0 && !dacl.is_null());
            unsafe { std::slice::from_raw_parts(dacl.cast(), (*dacl).AclSize as usize) }.to_vec()
        }
        // Never print SID-bearing ACL bytes on a failed equality assertion.
        assert!(
            acl_bytes(&unlabelled) == acl_bytes(&labelled),
            "fixture DACL grants changed"
        );
    }
    #[test]
    fn fixture_refuses_existing_directory_without_locking_it() {
        let tree = tempfile::tempdir().unwrap();
        std::fs::write(tree.path().join("sentinel"), b"unchanged").unwrap();
        assert!(rewrite_attempt(tree.path()).is_err());
        assert_eq!(
            std::fs::read(tree.path().join("sentinel")).unwrap(),
            b"unchanged"
        );
        assert!(std::fs::read_dir(tree.path()).is_ok());
    }
}
