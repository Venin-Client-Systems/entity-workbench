//! A narrowly accepted DOS spelling for JVM launch text, never filesystem trust.
//! JDK 21's native canonicalizer treats '?' in a verbatim prefix as a wildcard.
//! The native caller must additionally prove canonical identity before use.
use super::{bounded, Result};

pub(crate) fn dos_spelling(source: &str) -> Result<String> {
    let path = source.strip_prefix(r"\\?\").unwrap_or(source);
    let bytes = path.as_bytes();
    bounded(
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\',
        "Java launch requires a local drive path",
    )?;
    bounded(
        path.encode_utf16().count() < 260,
        "Java DOS launch path exceeds bound",
    )?;
    for component in path[3..].split('\\') {
        let stem = component
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end_matches(' ')
            .to_ascii_uppercase();
        bounded(
            !component.is_empty()
                && !component.ends_with(['.', ' '])
                && !component
                    .chars()
                    .any(|c| c.is_control() || r#"<>:"/|?*"#.contains(c))
                && !matches!(
                    stem.as_str(),
                    "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                )
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
                && !matches!(
                    stem.as_str(),
                    "COM¹" | "COM²" | "COM³" | "LPT¹" | "LPT²" | "LPT³"
                ),
            "Java launch path alias or component rejected",
        )?;
    }
    Ok(path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dos_spelling_is_only_for_unambiguous_bounded_local_drive_paths() {
        assert_eq!(
            dos_spelling(r"\\?\C:\assigned\java\bin\java.exe").unwrap(),
            r"C:\assigned\java\bin\java.exe"
        );
        assert_eq!(
            dos_spelling(r"D:\space allowed\file.txt").unwrap(),
            r"D:\space allowed\file.txt"
        );
        for path in [
            r"\\?\UNC\server\share\file",
            r"\\server\share\file",
            r"\\.\C:\file",
            r"\\?\Volume{0000}\file",
            r"\??\C:\file",
            r"C:relative",
            r"relative\file",
            r"C:\parent\..\file",
            r"C:\parent\.\file",
            r"C:\parent.\file",
            r"C:\parent \file",
            r"C:\file.",
            r"C:\file ",
            r"C:\file:stream",
            r"C:\parent\\file",
            r"C:/file",
            r"C:\p/file",
            r"C:\*.txt",
            r"C:\?.txt",
            r"C:\NUL",
            r"C:\CON.txt",
            r"C:\COM1.log",
            r"C:\LPT9",
            r"C:\CONIN$",
            r"C:\AUX .txt",
            r"C:\COM¹",
            r"C:\LPT².txt",
            "C:\\private\0name",
        ] {
            assert!(
                dos_spelling(path).is_err(),
                "unsafe synthetic spelling accepted"
            );
        }
        assert!(dos_spelling(&format!("C:\\{}", "x".repeat(257))).is_err());
        assert!(dos_spelling(&format!("C:\\{}", "😀".repeat(129))).is_err());
    }
}
