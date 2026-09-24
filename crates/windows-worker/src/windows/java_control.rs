//! Synthetic FileWorker startup control only. Never a production retry/fallback.
//! Only the compiled notice fixture is permitted. This deliberately has no
//! AppContainer token/ACL grant, but preserves the reviewed recipe, environment,
//! detached/no-inherited-handle launch and Job Object/time/handle/disk limits.
use super::*;
use crate::java::{self, diagnostics::FailureDiagnostics, JavaOutput, Job};

pub(crate) fn file_worker_control(
    runtime: &Path,
    parent: &Path,
    diagnostics: &mut FailureDiagnostics,
) -> Result<JavaOutput> {
    let job = Job::parse(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/parser/notice.txt"
        ))
        .to_vec(),
    )?;
    let mut prepared = java::prepare(runtime, parent, &job)?;
    prepared
        .request
        .arguments
        .insert(0, "-Dworkbench.probe=true".into());
    validate(&prepared.request)?;
    prepared.verify_runtime(runtime)?;
    ordinary(parent)?;
    let temporary = tempfile::Builder::new()
        .prefix("file-worker-control-")
        .tempdir_in(parent)?;
    let root = temporary.path().canonicalize()?;
    let owner = user_sid()?;
    let mut quiescent = true;
    let outcome = (|| {
        let input = root.join("input.json");
        let metadata = root.join("request.json");
        let scratch = root.join("scratch");
        fs::write(&input, &prepared.request.input)?;
        fs::write(&metadata, &prepared.metadata)?;
        fs::create_dir(&scratch)?;
        let executable = runtime.join(&prepared.request.executable);
        let path_text = java_paths::launch_text;
        let replacements = [
            ("$EW_INPUT", path_text(&input)?),
            ("$EW_REQUEST", path_text(&metadata)?),
            ("$EW_SCRATCH", path_text(&scratch)?),
            ("$EW_RUNTIME", path_text(runtime)?),
        ];
        let mut arguments = vec![path_text(&executable)?];
        for argument in &prepared.request.arguments {
            let mut argument = argument.clone();
            for (key, value) in &replacements {
                argument = argument.replace(key, value);
            }
            arguments.push(argument);
        }
        let command = arguments
            .iter()
            .map(|arg| quote_argument(arg))
            .collect::<Vec<_>>()
            .join(" ");
        blocked(
            command.encode_utf16().count() < 30000,
            "control command exceeds bound",
        )?;
        let mut command = wide(command)?;
        let executable = wide(path_text(&executable)?)?;
        let scratch_text = path_text(&scratch)?;
        let current_dir = wide(&scratch_text)?;
        let environment = worker_environment(&os_environment()?, Path::new(&scratch_text))?;
        let children: u32 = PROCESS_CREATION_CHILD_PROCESS_RESTRICTED;
        let mut attributes = Attributes::new(1)?;
        attributes.set(PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY, &children)?;
        let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.lpAttributeList = attributes.ptr();
        let job_object = create_job(prepared.request.memory_bytes)?;
        let mut process: PROCESS_INFORMATION = unsafe { zeroed() };
        // All buffers, startup attributes and environment storage live across
        // this synchronous call; no handle is inherited. The child is suspended
        // until assignment to the bounded kill-on-close Job Object succeeds.
        api(
            unsafe {
                CreateProcessW(
                    executable.as_ptr(),
                    command.as_mut_ptr(),
                    null(),
                    null(),
                    0,
                    EXTENDED_STARTUPINFO_PRESENT
                        | CREATE_SUSPENDED
                        | CREATE_UNICODE_ENVIRONMENT
                        | DETACHED_PROCESS,
                    environment.as_ptr().cast(),
                    current_dir.as_ptr(),
                    &startup.StartupInfo,
                    &mut process,
                )
            },
            "CreateSyntheticFileControl",
        )?;
        quiescent = false;
        let thread = Handle(process.hThread);
        let mut running = Running {
            process: Handle(process.hProcess),
            job: job_object,
            stopped: false,
        };
        if unsafe { AssignProcessToJobObject(running.job.0, running.process.0) } == 0 {
            let failure = Error::Api {
                operation: "AssignControlToJob",
                code: unsafe { GetLastError() },
            };
            // No code has run and Job Object assignment did not succeed.
            unsafe {
                TerminateProcess(running.process.0, 1);
            }
            quiescent = unsafe { WaitForSingleObject(running.process.0, 5000) } == WAIT_OBJECT_0;
            running.stopped = quiescent;
            return Err(failure);
        }
        let result = (|| {
            let token = token(running.process.0)?;
            let app = token_info(token.0, TokenIsAppContainer)?;
            blocked(
                unsafe { *app.as_ptr().cast::<u32>() } == 0,
                "control unexpectedly confined",
            )?;
            drop(token);
            blocked(
                unsafe { ResumeThread(thread.0) } != u32::MAX,
                "control resume failed",
            )?;
            drop(thread);
            let start = Instant::now();
            loop {
                blocked(
                    start.elapsed() < prepared.request.wall_time,
                    "control wall-time exceeded",
                )?;
                walk(&scratch, WRITABLE_LIMIT, 512)?;
                let mut handles = 0;
                api(
                    unsafe { GetProcessHandleCount(running.process.0, &mut handles) },
                    "ControlHandleCount",
                )?;
                blocked(handles <= HANDLE_LIMIT, "control handle budget exceeded")?;
                match unsafe { WaitForSingleObject(running.process.0, 20) } {
                    WAIT_OBJECT_0 => break,
                    WAIT_TIMEOUT => {}
                    _ => return Err(Error::Blocked("control wait failed")),
                }
            }
            walk(&scratch, WRITABLE_LIMIT, 512)?;
            let mut code = 0;
            api(
                unsafe { GetExitCodeProcess(running.process.0, &mut code) },
                "ControlExitCode",
            )?;
            if code != 0 {
                return Err(Error::Exit(code));
            }
            blocked(
                read_output_bounded(&input, 16 * 1024 * 1024)? == prepared.request.input
                    && read_output_bounded(&metadata, 1024 * 1024)? == prepared.metadata,
                "control assigned input changed",
            )?;
            let output = java::accept(
                &job,
                read_output_bounded(&scratch.join("result.json"), java::PARSE_BYTES)?,
                vec![],
            )?;
            let reply: serde_json::Value = serde_json::from_slice(&output.bytes)
                .map_err(|_| Error::Blocked("control parser schema rejected"))?;
            blocked(
                reply["status"] == "complete"
                    && reply["text"].as_str().is_some_and(|text| {
                        text.contains("Rowan Ellis")
                            && text.contains("Fictional Harbour Cooperative")
                    }),
                "control parser text/status mismatch",
            )?;
            Ok(output)
        })();
        if running.stop().is_err() {
            return Err(Error::Cleanup {
                prior: result.err().map(Box::new),
            });
        }
        quiescent = true;
        // Both success and failure retain the final fixed checkpoint; no raw
        // JVM output is exported. Collection occurs only after reaping.
        *diagnostics = java_diagnostics::capture_control(&scratch);
        result
    })();
    let path = temporary.keep();
    if !quiescent || clean(&path, &owner).is_err() {
        return Err(Error::Cleanup {
            prior: outcome.err().map(Box::new),
        });
    }
    outcome
}
