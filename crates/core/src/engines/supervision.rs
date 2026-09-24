//! Narrow development-only macOS launcher. Not the signed-helper release boundary.
use crate::{require, Error, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

const FILE_BYTES: u64 = 64 * 1024 * 1024;
const TREE_BYTES: u64 = 128 * 1024 * 1024;
const TREE_FILES: usize = 512;

// Lucene indexes are flat. Never grant an index containing links or special files.
pub(super) fn validate_index(index: &Path) -> Result<()> {
    require(
        fs::symlink_metadata(index)?.is_dir(),
        "Invalid index directory",
    )?;
    check_tree(index, 0, &mut 0, &mut 0)
}

/// Called only by the coordinator with its index lock held and no live worker.
/// Repair directory permissions only; never chmod a file, hard link or symlink.
pub(super) fn cleanup_tree(path: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata.is_dir() {
        let mut pending = vec![path.to_owned()];
        while let Some(directory) = pending.pop() {
            let name = CString::new(directory.as_os_str().as_bytes())
                .map_err(|_| Error::Validation("Invalid cleanup path".into()))?;
            // SAFETY: the NUL-terminated path remains alive for this syscall.
            // AT_SYMLINK_NOFOLLOW prevents chmod from following a replacement link.
            // Worker execution has ended; concurrent same-user tampering is excluded.
            let result = unsafe {
                libc::fchmodat(
                    libc::AT_FDCWD,
                    name.as_ptr(),
                    0o700,
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            };
            if result != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            for entry in fs::read_dir(&directory)? {
                let entry = entry?;
                if fs::symlink_metadata(entry.path())?.is_dir() {
                    pending.push(entry.path());
                }
            }
        }
        // Rust's directory removal does not traverse symlinks.
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    require(
        matches!(fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound),
        "Worker directory cleanup could not be verified",
    )
}

pub(super) fn finish_job<T>(job: tempfile::TempDir, result: Result<T>) -> Result<T> {
    // Take ownership explicitly so TempDir::drop cannot silently discard a failure.
    let path = job.keep();
    match cleanup_tree(&path) {
        Ok(()) => result,
        Err(cleanup) => {
            let preceding = match result {
                Ok(_) => "worker completed".into(),
                Err(error) => error.to_string(),
            };
            Err(Error::Blocked(format!(
                "Worker scratch cleanup failed; result rejected ({preceding}; cleanup: {cleanup})"
            )))
        }
    }
}

fn check_tree(path: &Path, depth: usize, count: &mut usize, bytes: &mut u64) -> Result<()> {
    require(depth <= 2, "Worker output nesting limit exceeded")?;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(value) => value,
            // Lucene removes temporary segment files between directory enumeration
            // and stat. The final scan runs only after the whole worker group exits.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        *count += 1;
        require(*count <= TREE_FILES, "Worker file-count limit exceeded")?;
        require(!metadata.file_type().is_symlink(), "Worker link rejected")?;
        if metadata.is_dir() {
            check_tree(&entry.path(), depth + 1, count, bytes)?;
        } else {
            require(
                metadata.is_file() && metadata.nlink() == 1,
                "Worker special or hard-linked file rejected",
            )?;
            *bytes = bytes
                .checked_add(metadata.len())
                .ok_or_else(|| Error::Validation("Worker output size overflow".into()))?;
            require(
                metadata.len() <= FILE_BYTES && *bytes <= TREE_BYTES,
                "Worker disk budget exceeded",
            )?;
        }
    }
    Ok(())
}

fn quote(path: &Path) -> Result<String> {
    let value = path
        .to_str()
        .ok_or_else(|| Error::Validation("Worker paths must be UTF-8".into()))?;
    require(
        !value.chars().any(char::is_control),
        "Control character in worker path",
    )?;
    Ok(serde_json::to_string(value)?)
}

fn profile(
    java: &Path,
    runtime: &Path,
    job: &Path,
    index: &Path,
    writable_index: bool,
) -> Result<String> {
    // The explicit deny of process-fork is intentional: Java threads work without it.
    // sandbox-exec is an experimental development mechanism, not our release helper.
    let mut result = format!(
        "(version 1)\n(deny default)\n(import \"dyld-support.sb\")\n(deny process-fork)\n(allow sysctl-read)\n(allow file-read-metadata)\n(allow process-exec (literal {}))\n(allow file-read* file-map-executable (subpath {}) (subpath {}) (subpath \"/usr/lib\") (subpath \"/System/Library\"))\n(allow file-read* (literal \"/dev/random\") (literal \"/dev/urandom\") (literal \"/dev/null\") (literal {}) (literal {}) (subpath {}) (subpath {}))\n(allow file-write* (literal {}) (subpath {}))\n",
        quote(java)?, quote(&runtime.join("java"))?, quote(&runtime.join("search"))?,
        quote(&job.join("input.json"))?, quote(&job.join("request.json"))?,
        quote(&job.join("scratch"))?, quote(index)?,
        quote(&job.join("result.json"))?, quote(&job.join("scratch"))?,
    );
    result.push_str(&format!("(allow file-read* (literal {}))\n", quote(job)?));
    if writable_index {
        result.push_str(&format!(
            "(allow file-write* (subpath {}))\n",
            quote(index)?
        ));
    }
    Ok(result)
}

/// All child setup is allocated before fork. The hook uses only libc syscalls and
/// OS error construction; no heap, locks, logging, or environment reads after fork.
fn configure_process(command: &mut Command) -> Result<()> {
    // Use the kernel's descriptor ceiling, not just the soft limit: an inherited
    // descriptor can be above a subsequently lowered soft limit.
    let mut max_files: libc::c_int = 0;
    let mut size = std::mem::size_of_val(&max_files);
    let name = c"kern.maxfilesperproc";
    // SAFETY: writable buffers have exactly the sizes provided to sysctlbyname.
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut max_files as *mut libc::c_int).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    require(
        max_files > 0 && max_files <= 1_048_576,
        "Unsupported descriptor ceiling",
    )?;
    command.process_group(0);
    // SAFETY: the child hook only invokes async-signal-safe syscalls; it preserves
    // Rust's exec error pipe until exec by marking descriptors rather than closing.
    unsafe {
        command.pre_exec(move || {
            for fd in 3..max_files {
                if libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) == -1 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::EBADF) {
                        return Err(error);
                    }
                }
            }
            for (resource, limit) in [
                (libc::RLIMIT_CORE, 0),
                (libc::RLIMIT_FSIZE, FILE_BYTES),
                (libc::RLIMIT_CPU, 30),
                (libc::RLIMIT_NOFILE, 256),
            ] {
                let value = libc::rlimit {
                    rlim_cur: limit,
                    rlim_max: limit,
                };
                if libc::setrlimit(resource, &value) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    Ok(())
}

struct ProcessGroup {
    child: Child,
    reaped: bool,
}
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        // SAFETY: this PID is also the dedicated group ID established before exec.
        // The guard remains alive until child exit. Fork is denied by the profile;
        // group cleanup also covers abnormal launcher failures and test descendants.
        unsafe {
            libc::kill(-(self.child.id() as libc::pid_t), libc::SIGKILL);
        }
        // Also signal the tracked leader directly if it changed process groups.
        // Fork is denied for the Java worker; general escaped descendants remain
        // outside this development mechanism's demonstrated containment claims.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn wait(child: Child, job: &Path, index: &Path, timeout: Duration) -> Result<ExitStatus> {
    let mut group = ProcessGroup {
        child,
        reaped: false,
    };
    let started = Instant::now();
    loop {
        // WNOWAIT preserves the leader PID until group termination, avoiding a
        // signal to a recycled PID after std::Child::try_wait has reaped it.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let observed = unsafe {
            libc::waitid(
                libc::P_PID,
                group.child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if observed != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if info.si_pid != 0 {
            unsafe {
                libc::kill(-(group.child.id() as libc::pid_t), libc::SIGKILL);
            }
            // The guard must not signal after reap. Mark it disarmed after wait.
            let status = group.child.wait()?;
            group.reaped = true;
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            return Err(Error::Blocked("Local search worker timed out".into()));
        }
        let mut count = 0;
        let mut bytes = 0;
        check_tree(job, 0, &mut count, &mut bytes)?;
        check_tree(index, 0, &mut count, &mut bytes)?;
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(super) fn run_java(
    runtime: &Path,
    job: &Path,
    index: &Path,
    writable_index: bool,
    class: &str,
    args: &[String],
    timeout: Duration,
) -> Result<()> {
    let runtime = runtime
        .canonicalize()
        .map_err(|_| Error::Blocked("Packaged Java runtime is unavailable".into()))?;
    let java = runtime.join("java/bin/java");
    require(
        java.is_file() && runtime.join("search/workers-0.1.0.jar").is_file(),
        "Packaged Java or worker JAR is missing",
    )?;
    fs::create_dir(job.join("scratch"))?;
    let profile_path = job.join("worker.sb");
    super::write_new(
        &profile_path,
        profile(&java, &runtime, job, index, writable_index)?.as_bytes(),
    )?;
    let classpath = format!(
        "{}:{}",
        runtime.join("search/workers-0.1.0.jar").display(),
        runtime.join("search/lib/*").display()
    );
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command
        .arg("-f")
        .arg(&profile_path)
        .arg(&java)
        .args([
            "-Xmx256m",
            "-XX:-UsePerfData",
            "-XX:+DisableAttachMechanism",
        ])
        .arg(format!(
            "-Djava.io.tmpdir={}",
            job.join("scratch").display()
        ))
        .arg(format!("-Duser.home={}", job.join("scratch").display()))
        .arg(format!("-Dworkbench.index={}", index.display()))
        .args(["-cp", &classpath, class])
        .args(args)
        .current_dir(job)
        .env_clear()
        // sandbox-exec on the observed macOS host fails without the actual HOME.
        // This value grants no file access; Java user.home points at job scratch.
        .envs(std::env::vars_os().filter(|(key, _)| key == "HOME"))
        .stdin(File::open(job.join("request.json"))?)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_process(&mut command)?;
    let status = wait(command.spawn()?, job, index, timeout)?;
    require(
        status.success(),
        "Local search worker failed; no result was accepted",
    )?;
    // Always inspect final files too: fast workers can exit between polls.
    let mut count = 0;
    let mut bytes = 0;
    check_tree(job, 0, &mut count, &mut bytes)?;
    check_tree(index, 0, &mut count, &mut bytes)?;
    Ok(())
}

pub(super) fn read_result(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    require(
        metadata.is_file() && metadata.nlink() == 1 && metadata.len() <= limit,
        "Worker returned an invalid result file",
    )?;
    let mut bytes = Vec::new();
    (&mut file).take(limit + 1).read_to_end(&mut bytes)?;
    require(
        bytes.len() as u64 <= limit,
        "Worker result exceeded its limit",
    )?;
    Ok(bytes)
}

#[cfg(test)]
mod tests;
