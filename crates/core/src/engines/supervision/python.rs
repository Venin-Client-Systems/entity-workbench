//! Private fixed Python runner shared with historical test recipes. No runtime discovery.
use super::*;
use crate::engines::{ocr::digest, CancellationToken};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Component, PathBuf},
};

pub(in crate::engines) fn cancelled(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(Error::Interrupted("Local worker cancelled".into()))
    } else {
        Ok(())
    }
}

// The opened descriptor cannot block on a FIFO swapped after lstat. Every read
// verifies both the handle and final named path, using the immutable-file policy.
fn read_chunks(
    path: &Path,
    maximum: u64,
    cancel: &CancellationToken,
    mut consume: impl FnMut(&[u8]),
    mut boundary: impl FnMut(&Path, u64),
) -> Result<u64> {
    cancelled(cancel)?;
    let named = fs::symlink_metadata(path)?;
    require(
        named.is_file() && named.nlink() == 1,
        "Python file is not an ordinary single-link file",
    )?;
    enforce_limit(named.len() <= maximum, "Python file exceeds byte limit")?;
    boundary(path, 0);
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let opened = file.metadata()?;
    crate::store::file_identity::unchanged(&named, &opened)?;
    require(
        opened.is_file() && opened.nlink() == 1,
        "Python opened file is not ordinary",
    )?;
    let mut count = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        cancelled(cancel)?;
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        count = count
            .checked_add(read as u64)
            .ok_or_else(|| Error::QuotaExhausted("Python byte count overflow".into()))?;
        enforce_limit(count <= maximum, "Python file grew beyond byte limit")?;
        consume(&buffer[..read]);
        boundary(path, count);
    }
    cancelled(cancel)?;
    crate::store::file_identity::unchanged(&opened, &file.metadata()?)?;
    let final_named = fs::symlink_metadata(path)?;
    require(
        final_named.is_file() && final_named.nlink() == 1,
        "Python file replaced during read",
    )?;
    crate::store::file_identity::unchanged(&opened, &final_named)?;
    require(
        count == opened.len(),
        "Python file length changed during read",
    )?;
    Ok(count)
}

pub(in crate::engines) fn read_verified(
    path: &Path,
    maximum: u64,
    cancel: &CancellationToken,
) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    read_chunks(
        path,
        maximum,
        cancel,
        |bytes| data.extend_from_slice(bytes),
        |_, _| {},
    )?;
    Ok(data)
}

fn hash_verified(
    path: &Path,
    maximum: u64,
    cancel: &CancellationToken,
    boundary: impl FnMut(&Path, u64),
) -> Result<(u64, String)> {
    let mut hash = Sha256::new();
    let count = read_chunks(path, maximum, cancel, |bytes| hash.update(bytes), boundary)?;
    Ok((count, format!("{:x}", hash.finalize())))
}

pub(in crate::engines) type AssetInput<'a> = (&'static str, &'a [u8], usize);
#[derive(Serialize)]
pub(in crate::engines) struct Assigned {
    bytes: u64,
    sha256: String,
    #[serde(skip)]
    maximum: u64,
}

pub(in crate::engines) fn stage(
    job: &Path,
    assets: &[AssetInput<'_>],
    cancel: &CancellationToken,
) -> Result<BTreeMap<String, Assigned>> {
    let mut assigned = BTreeMap::new();
    for (name, data, maximum) in assets {
        cancelled(cancel)?;
        require(
            data.len() <= *maximum,
            "Compiled Python input exceeds reviewed bound",
        )?;
        require(
            Path::new(name)
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
            "Invalid assigned Python path",
        )?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(job.join(name))?;
        for chunk in data.chunks(64 * 1024) {
            cancelled(cancel)?;
            file.write_all(chunk)?;
        }
        file.sync_all()?;
        let raw = read_verified(&job.join(name), *maximum as u64, cancel)?;
        require(raw == *data, "Assigned Python bytes differ")?;
        assigned.insert(
            (*name).into(),
            Assigned {
                bytes: raw.len() as u64,
                sha256: digest(&raw),
                maximum: *maximum as u64,
            },
        );
    }
    Ok(assigned)
}

pub(in crate::engines) fn verify_assigned(
    job: &Path,
    assigned: &BTreeMap<String, Assigned>,
    cancel: &CancellationToken,
) -> Result<()> {
    for (name, expected) in assigned {
        let (bytes, sha256) = hash_verified(&job.join(name), expected.maximum, cancel, |_, _| {})?;
        require(
            bytes == expected.bytes && sha256 == expected.sha256,
            "Assigned Python input changed",
        )?;
    }
    Ok(())
}

// Entry/argument are selected exclusively by fixed Rust recipes, never supplied
// through IPC or worker JSON. Launch, inherited handles and wait/reap stay shared.
pub(in crate::engines) fn command(
    prefix: &Path,
    job: &Path,
    entry: &str,
    argument: Option<&str>,
) -> Result<Command> {
    let stdout = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(job.join("scratch/stdout.txt"))?;
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(job.join("scratch/stderr.txt"))?;
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command
        .arg("-f")
        .arg(job.join("worker.sb"))
        .arg(prefix.join("install/bin/python3.13"))
        .args(["-I", "-S", "-B"])
        .arg(job.join("code").join(entry));
    if let Some(argument) = argument {
        command.arg(argument);
    }
    command
        .current_dir(job.join("scratch"))
        .env_clear()
        .envs(std::env::vars_os().filter(|(key, _)| key == "HOME"))
        .env("TMPDIR", job.join("scratch"))
        .env("OMP_THREAD_LIMIT", "1")
        .env("OMP_NUM_THREADS", "1")
        .env("OPENBLAS_NUM_THREADS", "1")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr);
    configure_process(&mut command)?;
    Ok(command)
}

// No callback (including inventory/output reads) runs after uncertain termination.
fn after_confirmed<T>(
    status: Result<ExitStatus>,
    inspect: impl FnOnce(ExitStatus) -> Result<T>,
) -> Result<T> {
    // Failed/cancelled waits have no post-inventory claim. In particular, do not
    // force a long hash pass after cancellation merely to manufacture one.
    inspect(status?)
}

#[cfg(target_arch = "aarch64")]
pub(in crate::engines) fn execute_graph<T>(
    prefix: &Path,
    scratch_root: &Path,
    assets: &[AssetInput<'_>],
    cancel: &CancellationToken,
    accept: impl FnOnce(&Path) -> Result<T>,
) -> Result<T> {
    cancelled(cancel)?;
    validate_scratch_root(scratch_root)?;
    verify_prefix(prefix, MANIFEST, FILE_COUNT, cancel)
        .map_err(super::super::python_graph::unavailable)?;
    let job = tempfile::Builder::new()
        .prefix("graph-job-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(scratch_root)?;
    let result = (|| {
        for name in ["code", "input", "scratch"] {
            fs::create_dir(job.path().join(name))?;
        }
        let assigned = stage(job.path(), assets, cancel)?;
        let expanded = profile(prefix, job.path())?;
        super::super::write_new(&job.path().join("worker.sb"), expanded.as_bytes())?;
        cancelled(cancel)?;
        let mut command = command(prefix, job.path(), "graph_worker.py", None)?;
        cancelled(cancel)?;
        let child = command.spawn()?;
        let status = wait_assigned_with_cancellation(
            child,
            job.path(),
            None,
            Duration::from_secs(30),
            Some(cancel),
            || Error::Interrupted("Local worker cancelled".into()),
        );
        after_confirmed(status, |status| {
            // Cancellation and supervisor failures cannot become successful output.
            cancelled(cancel)?;
            check_tree(job.path(), 0, &mut 0, &mut 0)?;
            verify_assigned(job.path(), &assigned, cancel)?;
            verify_prefix(prefix, MANIFEST, FILE_COUNT, cancel)?;
            if !status.success() {
                return Err(Error::InvalidWorkerResult(
                    "Fixed graph worker failed".into(),
                ));
            }
            let result = accept(job.path())?;
            cancelled(cancel)?;
            Ok(result)
        })
    })();
    finish_job(job, result)
}

#[cfg(test)]
mod tests;

#[cfg(target_arch = "aarch64")]
pub(in crate::engines) const MANIFEST: &str =
    "4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822";
#[cfg(target_arch = "aarch64")]
pub(in crate::engines) const FILE_COUNT: usize = 11_320;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Asset {
    bytes: u64,
    sha256: String,
    executable: bool,
}
pub(in crate::engines) fn profile(prefix: &Path, job: &Path) -> Result<String> {
    Ok(format!(
        "(version 1)\n(deny default)\n(import \"dyld-support.sb\")\n(deny process-fork)\n(allow sysctl-read)\n(allow file-read-metadata)\n(allow process-exec (literal {}))\n(allow file-read* file-map-executable (subpath {}) (subpath \"/usr/lib\") (subpath \"/System/Library\"))\n(allow file-read* (literal {}) (subpath {}) (subpath {}) (subpath {}) (literal \"/dev/null\") (literal \"/dev/random\") (literal \"/dev/urandom\"))\n(allow file-write* (subpath {}))\n",
        quote(&prefix.join("install/bin/python3.13"))?, quote(prefix)?, quote(job)?,
        quote(&job.join("code"))?, quote(&job.join("input"))?, quote(&job.join("scratch"))?, quote(&job.join("scratch"))?
    ))
}

pub(in crate::engines) fn reject_linked_ancestors(path: &Path) -> Result<()> {
    require(path.is_absolute(), "Probe paths must be absolute")?;
    let mut current = PathBuf::new();
    for part in path.components() {
        require(
            matches!(part, Component::RootDir | Component::Normal(_)),
            "Invalid probe path",
        )?;
        current.push(part);
        require(
            !fs::symlink_metadata(&current)?.file_type().is_symlink(),
            "Linked probe ancestor rejected",
        )?;
    }
    Ok(())
}

pub(in crate::engines) fn verify_prefix(
    prefix: &Path,
    expected_manifest: &str,
    expected_count: usize,
    cancel: &CancellationToken,
) -> Result<()> {
    cancelled(cancel)?;
    reject_linked_ancestors(prefix)?;
    let raw = read_verified(&prefix.join("manifest.json"), 16 * 1024 * 1024, cancel)?;
    require(
        digest(&raw) == expected_manifest,
        "Probe runtime manifest identity mismatch",
    )?;
    let value: serde_json::Value = serde_json::from_slice(&raw)?;
    let files = value
        .get("files")
        .ok_or_else(|| Error::Validation("Probe file inventory missing".into()))?;
    let mut assets: BTreeMap<String, Asset> = serde_json::from_value(files.clone())?;
    require(
        assets.len() + 1 == expected_count && expected_count <= 20_000,
        "Probe runtime file count mismatch",
    )?;
    assets.insert(
        "manifest.json".into(),
        Asset {
            bytes: raw.len() as u64,
            sha256: expected_manifest.into(),
            executable: false,
        },
    );
    let mut actual = BTreeSet::new();
    let mut pending = vec![(prefix.to_owned(), 0usize)];
    let mut entries = 0;
    while let Some((directory, depth)) = pending.pop() {
        require(depth <= 16, "Probe runtime depth exceeded")?;
        cancelled(cancel)?;
        for item in fs::read_dir(directory)? {
            cancelled(cancel)?;
            let item = item?;
            entries += 1;
            require(entries <= 40_000, "Probe runtime entry count exceeded")?;
            let info = fs::symlink_metadata(item.path())?;
            require(
                !info.file_type().is_symlink(),
                "Linked probe runtime entry rejected",
            )?;
            if info.is_dir() {
                pending.push((item.path(), depth + 1));
            } else {
                require(
                    info.is_file() && info.nlink() == 1,
                    "Special or hardlinked probe runtime entry",
                )?;
                let path = item.path();
                let relative = path
                    .strip_prefix(prefix)
                    .map_err(|_| Error::Validation("Invalid runtime path".into()))?;
                actual.insert(
                    relative
                        .to_str()
                        .ok_or_else(|| Error::Validation("Non-UTF8 runtime path".into()))?
                        .to_owned(),
                );
            }
        }
    }
    require(
        actual == assets.keys().cloned().collect(),
        "Probe runtime has missing or unlisted assets",
    )?;
    let mut total = 0u64;
    for (name, asset) in assets {
        require(
            Path::new(&name)
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
            "Invalid runtime asset path",
        )?;
        total = total
            .checked_add(asset.bytes)
            .ok_or_else(|| Error::Validation("Runtime size overflow".into()))?;
        require(
            asset.bytes <= 80 * 1024 * 1024 && total <= 768 * 1024 * 1024,
            "Probe runtime size exceeded",
        )?;
        cancelled(cancel)?;
        let path = prefix.join(name);
        let before = fs::symlink_metadata(&path)?;
        require(
            before.permissions().mode() & 0o7777 == if asset.executable { 0o755 } else { 0o644 },
            "Probe runtime asset metadata mismatch",
        )?;
        let (size, sha256) = hash_verified(&path, asset.bytes, cancel, |_, _| {})?;
        crate::store::file_identity::unchanged(&before, &fs::symlink_metadata(&path)?)?;
        require(
            size == asset.bytes && sha256 == asset.sha256,
            "Probe runtime integrity mismatch",
        )?;
    }
    Ok(())
}

fn validate_scratch_root(path: &Path) -> Result<()> {
    reject_linked_ancestors(path)?;
    let info = fs::symlink_metadata(path)?;
    // Only a private application-owned directory may receive an assignment.
    require(
        info.is_dir()
            && info.permissions().mode() & 0o7777 == 0o700
            && info.uid() == unsafe { libc::geteuid() },
        "Graph scratch root must be private and owned",
    )
}
