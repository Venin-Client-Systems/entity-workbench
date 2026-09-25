//! Fixed-argument native OCR assignment; never grants Java/parser/index access.
use super::*;
use crate::engines::{ocr::MAX_TEXT_BYTES, CancellationToken};

#[derive(Clone, Copy)]
pub(super) enum Recipe {
    Text,
    WordRegions,
}
impl Recipe {
    fn limit(self) -> u64 {
        match self {
            Self::Text => MAX_TEXT_BYTES,
            Self::WordRegions => crate::engines::ocr_regions::MAX_TSV_BYTES,
        }
    }
}

fn profile(binary: &Path, runtime: &Path, job: &Path, recipe: Recipe) -> Result<String> {
    let output = match recipe {
        Recipe::Text => format!("(literal {})", quote(&job.join("result.txt"))?),
        Recipe::WordRegions => format!(
            "(literal {}) (literal {})",
            quote(&job.join("result.txt"))?,
            quote(&job.join("result.tsv"))?,
        ),
    };
    Ok(format!(
        "(version 1)\n(deny default)\n(import \"dyld-support.sb\")\n(deny process-fork)\n(allow sysctl-read)\n(allow file-read-metadata)\n(allow process-exec (literal {}))\n(allow file-read* file-map-executable (literal {}) (subpath {}) (subpath \"/usr/lib\") (subpath \"/System/Library\"))\n(allow file-read* (subpath {}) (literal {}) (literal {}) (subpath {}) (literal \"/dev/null\") (literal \"/dev/random\") (literal \"/dev/urandom\"))\n(allow file-write* {} (subpath {}))\n",
        quote(binary)?, quote(binary)?, quote(&runtime.join("lib"))?,
        quote(&runtime.join("tessdata"))?, quote(&job.join("input.pgm"))?, quote(job)?, quote(&job.join("scratch"))?,
        output, quote(&job.join("scratch"))?,
    ))
}

pub(crate) fn run(
    runtime: &Path,
    job: &Path,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<()> {
    run_recipe(runtime, job, timeout, cancellation, Recipe::Text)
}

pub(crate) fn run_regions(
    runtime: &Path,
    job: &Path,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<()> {
    run_recipe(runtime, job, timeout, cancellation, Recipe::WordRegions)
}

fn run_recipe(
    runtime: &Path,
    job: &Path,
    timeout: Duration,
    cancellation: &CancellationToken,
    recipe: Recipe,
) -> Result<()> {
    let binary = runtime.join("bin/tesseract");
    if !binary.is_file() || !runtime.join("tessdata/eng.traineddata").is_file() {
        return Err(Error::Blocked(
            "Packaged OCR executable or English model is unavailable".into(),
        ));
    }
    fs::create_dir(job.join("scratch"))?;
    let profile_path = job.join("worker.sb");
    super::super::write_new(
        &profile_path,
        profile(&binary, runtime, job, recipe)?.as_bytes(),
    )?;
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
    if matches!(recipe, Recipe::WordRegions) {
        command.args(["-c", "tessedit_create_tsv=1", "-c", "tessedit_create_txt=1"]);
    }
    configure_process(&mut command)?;
    let max_output = recipe.limit();
    // Tight native OCR output bound; shared process setup still supplies CPU/fd/core limits.
    unsafe {
        command.pre_exec(move || {
            let limit = libc::rlimit {
                rlim_cur: max_output,
                rlim_max: max_output,
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

#[cfg(test)]
#[path = "ocr/region_tests.rs"]
mod region_tests;
