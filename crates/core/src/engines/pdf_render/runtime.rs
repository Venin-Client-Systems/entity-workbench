//! Check the complete component inventory before assigning its read-only classpath.
use super::*;
use std::{collections::BTreeMap, fs, os::unix::fs::MetadataExt};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    renderer: String,
    files: BTreeMap<String, String>,
}
const JARS: [&str; 8] = [
    "workers-0.1.0.jar",
    "lib/pdfbox-3.0.8.jar",
    "lib/pdfbox-io-3.0.8.jar",
    "lib/fontbox-3.0.8.jar",
    "lib/commons-logging-1.4.0.jar",
    "lib/jackson-annotations-2.22.jar",
    "lib/jackson-core-2.22.3.jar",
    "lib/jackson-databind-2.22.3.jar",
];
pub(super) fn validate(root: &Path) -> Result<()> {
    let component = root.join("pdf-render");
    let metadata = fs::symlink_metadata(&component)
        .map_err(|_| Error::Blocked("Packaged PDF renderer is unavailable".into()))?;
    require(metadata.is_dir(), "Invalid PDF runtime directory")?;
    let encoded = super::super::supervision::read_result(&component.join("manifest.json"), 32768)?;
    let manifest: Manifest = serde_json::from_slice(&encoded)?;
    require(
        manifest.schema_version == 1
            && manifest.renderer == "pdfbox-3.0.8-scan-v1"
            && manifest.files.len() <= 64,
        "Invalid PDF runtime manifest",
    )?;
    let mut actual = BTreeMap::new();
    let mut pending = vec![(component.clone(), 0)];
    let mut bytes = 0;
    let mut entries = 0;
    while let Some((directory, depth)) = pending.pop() {
        require(depth <= 3, "PDF runtime inventory nesting limit")?;
        for entry in fs::read_dir(directory)? {
            entries += 1;
            require(entries <= 128, "PDF runtime inventory entry limit")?;
            let path = entry?.path();
            let meta = fs::symlink_metadata(&path)?;
            require(!meta.file_type().is_symlink(), "Linked PDF runtime asset")?;
            if meta.is_dir() {
                pending.push((path, depth + 1));
            } else {
                require(
                    meta.is_file() && meta.nlink() == 1,
                    "Special or hard-linked PDF runtime asset",
                )?;
                let name = path
                    .strip_prefix(&component)
                    .map_err(|_| Error::Validation("Invalid PDF runtime path".into()))?
                    .to_str()
                    .ok_or_else(|| Error::Validation("Non UTF-8 PDF runtime path".into()))?
                    .to_owned();
                if name == "manifest.json" {
                    continue;
                }
                bytes += meta.len();
                require(
                    actual.len() < 64 && bytes <= 64 * 1024 * 1024,
                    "PDF runtime inventory size limit",
                )?;
                require(
                    JARS.contains(&name.as_str()) || name.starts_with("notices/"),
                    "Unexpected PDF runtime asset",
                )?;
                actual.insert(
                    name,
                    digest(&super::super::supervision::read_result(
                        &path,
                        16 * 1024 * 1024,
                    )?),
                );
            }
        }
    }
    require(
        JARS.iter().all(|name| actual.contains_key(*name)) && actual == manifest.files,
        "PDF runtime inventory or content mismatch",
    )?;
    // Notices are retained with the exact JAR closure; staging reports their identities.
    require(
        actual
            .keys()
            .any(|name| name.starts_with("notices/pdfbox-3.0.8/")),
        "PDF dependency notices missing",
    )
}
