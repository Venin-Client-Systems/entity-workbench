//! Private verification shared by export and immutable-object readers.
use crate::{require, Result};
use std::fs;
#[cfg(windows)]
use std::{fs::File, os::windows::io::AsRawHandle};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
    BY_HANDLE_FILE_INFORMATION, FILE_ID_INFO,
};
pub(crate) fn unchanged(before: &fs::Metadata, after: &fs::Metadata) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        require(
            before.dev() == after.dev()
                && before.ino() == after.ino()
                && before.ctime() == after.ctime()
                && before.ctime_nsec() == after.ctime_nsec()
                && before.nlink() == after.nlink(),
            "Retained file identity or metadata changed during read",
        )?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        require(
            before.creation_time() == after.creation_time()
                && before.last_write_time() == after.last_write_time()
                && before.file_attributes() == after.file_attributes(),
            "Retained file metadata changed during read",
        )?;
    }
    require(
        before.len() == after.len() && before.modified()? == after.modified()?,
        "Retained file changed during read",
    )
}
#[cfg(windows)]
#[derive(PartialEq, Eq)]
pub(super) struct Identity {
    volume: u64,
    index: [u8; 16],
    links: u32,
}
#[cfg(windows)]
pub(super) fn windows_handle(file: &File, links: u64) -> Result<Identity> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // The File owns a live synchronous file handle throughout the call; info is
    // a writable correctly sized native structure and does not escape this call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    require(
        info.nNumberOfLinks as u64 == links,
        "Retained file has unexpected hard links",
    )?;
    // FileIdInfo supplies the full 128-bit identity (the legacy 64-bit file
    // index is not guaranteed unique on ReFS). Unsupported queries fail closed.
    let mut full: FILE_ID_INFO = unsafe { std::mem::zeroed() };
    // Same live handle and correctly sized writable native structure as above.
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
        links: info.nNumberOfLinks,
    })
}
