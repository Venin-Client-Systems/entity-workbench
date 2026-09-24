//! Stable Win32 handle identity/link count absent from stable std MetadataExt.
use crate::{require, Result};
use std::{fs::File, os::windows::io::AsRawHandle};
use windows_sys::Win32::Storage::FileSystem::{
    FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
    BY_HANDLE_FILE_INFORMATION, FILE_ID_INFO,
};
#[derive(PartialEq, Eq)]
pub(super) struct Identity {
    volume: u64,
    index: [u8; 16],
    links: u32,
}
pub(super) fn identity(file: &File, links: u64) -> Result<Identity> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // The File owns a live synchronous file handle throughout the call; info is
    // a writable correctly sized native structure and does not escape this call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    require(
        info.nNumberOfLinks as u64 == links,
        "Export has unexpected hard links",
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
