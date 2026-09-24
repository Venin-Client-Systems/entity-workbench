//! Fixed-argument native OCR assignment; never grants Java/parser/index access.
use super::*;
use crate::engines::{ocr::MAX_TEXT_BYTES, CancellationToken};

fn profile(binary: &Path, runtime: &Path, job: &Path) -> Result<String> {
    Ok(format!(
        "(version 1)\n(deny default)\n(import \"dyld-support.sb\")\n(deny process-fork)\n(allow sysctl-read)\n(allow file-read-metadata)\n(allow process-exec (literal {}))\n(allow file-read* file-map-executable (literal {}) (subpath {}) (subpath \"/usr/lib\") (subpath \"/System/Library\"))\n(allow file-read* (subpath {}) (literal {}) (literal {}) (subpath {}) (literal \"/dev/null\") (literal \"/dev/random\") (literal \"/dev/urandom\"))\n(allow file-write* (literal {}) (subpath {}))\n",
        quote(binary)?, quote(binary)?, quote(&runtime.join("lib"))?,
        quote(&runtime.join("tessdata"))?, quote(&job.join("input.pgm"))?, quote(job)?, quote(&job.join("scratch"))?,
        quote(&job.join("result.txt"))?, quote(&job.join("scratch"))?,
    ))
}

pub(crate) fn run(
    runtime: &Path,
    job: &Path,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<()> {
    let binary = runtime.join("bin/tesseract");
    if !binary.is_file() || !runtime.join("tessdata/eng.traineddata").is_file() {
        return Err(Error::Blocked(
            "Packaged OCR executable or English model is unavailable".into(),
        ));
    }
    fs::create_dir(job.join("scratch"))?;
    let profile_path = job.join("worker.sb");
    super::super::write_new(&profile_path, profile(&binary, runtime, job)?.as_bytes())?;
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command
        .arg("-f")
        .arg(profile_path)
        .arg(binary)
        .arg(job.join("input.pgm"))
        .arg(job.join("result"))
        .arg("--tessdata-dir")
        .arg(runtime.join("tessdata"))
        .args(["-l", "eng", "--oem", "1", "--psm", "6", "--dpi", "300"])
        .current_dir(job)
        .env_clear()
        .envs(std::env::vars_os().filter(|(key, _)| key == "HOME"))
        .env("TMPDIR", job.join("scratch"))
        .env("OMP_THREAD_LIMIT", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_process(&mut command)?;
    // Tight native OCR output bound; shared process setup still supplies CPU/fd/core limits.
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: MAX_TEXT_BYTES,
                rlim_max: MAX_TEXT_BYTES,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    if cancellation.is_cancelled() {
        return Err(Error::Blocked("OCR cancelled".into()));
    }
    let status = wait_assigned(command.spawn()?, job, None, timeout, Some(cancellation))?;
    require(
        status.success(),
        "OCR worker failed; no result was accepted",
    )?;
    check_tree(job, 0, &mut 0, &mut 0)
}

#[cfg(test)]
mod tests;
