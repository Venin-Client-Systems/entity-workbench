//! Synthetic native acceptance harness. Never point it at real investigation data.
#[cfg(any(windows, test))]
#[path = "ew-windows-probe/handle_probe.rs"]
mod handle_probe;
#[cfg(windows)]
#[path = "ew-windows-probe/restricted_probe.rs"]
mod restricted_probe;
#[cfg(any(windows, test))]
#[path = "ew-windows-probe/udp_probe.rs"]
mod udp_probe;
#[cfg(windows)]
mod native {
    use super::handle_probe::{accept_handle_outcome, HandleObservation};
    use super::udp_probe::{accept_delivery_denial, exchange, Listener, Markers, UdpObservation};
    use serde::{Deserialize, Serialize};
    use std::{
        collections::BTreeMap,
        fs::{self, File},
        io::{Read, Seek, SeekFrom, Write},
        mem::{size_of, zeroed},
        net::{TcpListener, TcpStream},
        os::windows::{ffi::OsStrExt, io::AsRawHandle},
        path::{Path, PathBuf},
        ptr::{null, null_mut},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::*,
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
        System::{Console::GetConsoleProcessList, Threading::*},
    };
    use workbench_windows_worker::{
        protect_private_tree, run, run_probe, Error, ProbeCheckpoint, ProbeDiagnostics, Request,
    };
    type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;
    const SECRET: &[u8] = b"EW_HANDLE_SECRET";

    #[derive(Serialize, Deserialize)]
    struct Input {
        mode: String,
        other: PathBuf,
        original: PathBuf,
        tcp: String,
        dns: String,
        udp_marker: String,
        handle: usize,
    }
    fn checkpoint(value: ProbeCheckpoint) -> AnyResult<()> {
        fs::write("probe-checkpoint.json", serde_json::to_vec(&value)?)?;
        Ok(())
    }
    fn token_is_container() -> AnyResult<bool> {
        checkpoint(ProbeCheckpoint::TokenOpen)?;
        let mut token = null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Ok(false);
        }
        let (mut value, mut size) = (0u32, 0u32);
        checkpoint(ProbeCheckpoint::TokenQuery)?;
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenIsAppContainer,
                (&mut value as *mut u32).cast(),
                4,
                &mut size,
            )
        };
        checkpoint(ProbeCheckpoint::TokenClose)?;
        unsafe {
            CloseHandle(token);
        }
        Ok(ok != 0 && value == 1)
    }
    fn handle_read(handle: usize) -> AnyResult<HandleObservation> {
        let app_container = token_is_container()?;
        let mut bytes = [0u8; 16];
        let mut count = 0;
        // The trusted parent resets the real sentinel's offset before either
        // launch. The confined worker must not seek an untrusted numeric handle.
        checkpoint(if app_container {
            ProbeCheckpoint::ConfinedHandleRead
        } else {
            ProbeCheckpoint::InheritedHandleRead
        })?;
        let sentinel_read = (unsafe {
            ReadFile(
                handle as HANDLE,
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                &mut count,
                null_mut(),
            ) != 0
        }) && count == 16
            && bytes.as_slice() == SECRET;
        checkpoint(ProbeCheckpoint::HandleReadReturned)?;
        Ok(HandleObservation {
            app_container,
            sentinel_read,
        })
    }
    fn rewrite_dacl(path: &Path) -> bool {
        let path = utf16(path.as_os_str());
        let mut descriptor = null_mut();
        let mut dacl = null_mut();
        let read = unsafe {
            GetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            )
        };
        if read != 0 {
            return false;
        }
        let result = unsafe {
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                dacl,
                null(),
            )
        };
        unsafe {
            LocalFree(descriptor);
        }
        result == 0
    }
    fn child(path: &Path) -> AnyResult<()> {
        checkpoint(ProbeCheckpoint::ChildEntered)?;
        checkpoint(ProbeCheckpoint::InputRead)?;
        let input: Input = serde_json::from_slice(&fs::read(path)?)?;
        match input.mode.as_str() {
            "udp-control" => {
                checkpoint(ProbeCheckpoint::UdpProbe)?;
                let observation = exchange(input.dns.parse()?, &input.udp_marker)?;
                fs::write("result.json", serde_json::to_vec(&observation)?)?;
                checkpoint(ProbeCheckpoint::Completed)?;
                return Ok(());
            }
            "inherited-handle" => {
                let observation = handle_read(input.handle)?;
                fs::write("result.json", serde_json::to_vec(&observation)?)?;
                checkpoint(ProbeCheckpoint::Completed)?;
                return Ok(());
            }
            "timeout" => {
                std::thread::sleep(Duration::from_secs(120));
                return Ok(());
            }
            "disk" => {
                let mut file = File::create("oversize.bin")?;
                for _ in 0..128 {
                    file.write_all(&vec![1; 1024 * 1024])?;
                }
                fs::write("result.json", b"{}")?;
                return Ok(());
            }
            "memory" => {
                let mut blocks = Vec::new();
                for _ in 0..2048 {
                    blocks.push(vec![3u8; 1024 * 1024]);
                }
                std::hint::black_box(&blocks);
                std::thread::sleep(Duration::from_secs(120));
                return Ok(());
            }
            "handles" => {
                let mut handles = Vec::new();
                for _ in 0..4096 {
                    handles.push(unsafe { CreateEventW(null(), 0, 0, null()) });
                }
                std::hint::black_box(&handles);
                std::thread::sleep(Duration::from_secs(120));
                return Ok(());
            }
            "output-large" => {
                fs::write("result.json", vec![0; 1024 * 1024 + 1])?;
                return Ok(());
            }
            "output-hardlink" => {
                fs::write("alias.json", b"{}")?;
                fs::hard_link("alias.json", "result.json")?;
                return Ok(());
            }
            "named-stream" => {
                fs::write("small.txt", b"small")?;
                fs::write("small.txt:payload", vec![0; 1024 * 1024])?;
                fs::write("result.json", b"{}")?;
                return Ok(());
            }
            "restricted-directory" => {
                drop(super::restricted_probe::create(
                    Path::new("restricted"),
                    checkpoint,
                )?);
                checkpoint(ProbeCheckpoint::RestrictedResultWrite)?;
                fs::write("result.json", b"{}")?;
                checkpoint(ProbeCheckpoint::Completed)?;
                return Ok(());
            }
            "readonly-file" => {
                fs::write("locked.txt", b"synthetic readonly")?;
                let mut permissions = fs::metadata("locked.txt")?.permissions();
                permissions.set_readonly(true);
                fs::set_permissions("locked.txt", permissions)?;
                fs::write("result.json", b"{}")?;
                return Ok(());
            }
            "probe" => {}
            _ => return Err("unsupported synthetic mode".into()),
        }
        let mut results = BTreeMap::new();
        results.insert("app_container", token_is_container()?);
        // Retain only attachment state, never PIDs. Baseline intentionally uses
        // CREATE_NO_WINDOW; the confined launcher must use DETACHED_PROCESS.
        let mut console_process = 0;
        checkpoint(ProbeCheckpoint::ConsoleQuery)?;
        results.insert("console_attached", unsafe {
            GetConsoleProcessList(&mut console_process, 1) > 0
        });
        checkpoint(ProbeCheckpoint::OtherWorkspaceRead)?;
        results.insert("other_workspace_read", fs::read(&input.other).is_ok());
        checkpoint(ProbeCheckpoint::OriginalWrite)?;
        results.insert(
            "original_write",
            fs::write(&input.original, b"modified").is_ok(),
        );
        checkpoint(ProbeCheckpoint::OriginalDacl)?;
        results.insert("original_dacl", rewrite_dacl(&input.original));
        checkpoint(ProbeCheckpoint::InputDacl)?;
        results.insert("input_dacl", rewrite_dacl(path));
        checkpoint(ProbeCheckpoint::RuntimeDacl)?;
        results.insert(
            "runtime_dacl",
            rewrite_dacl(
                &std::env::current_exe()?
                    .parent()
                    .ok_or("no parent")?
                    .join("runtime.txt"),
            ),
        );
        checkpoint(ProbeCheckpoint::AssignedInputRead)?;
        results.insert("input_read", fs::read(path).is_ok());
        checkpoint(ProbeCheckpoint::AssignedInputWrite)?;
        results.insert("input_write", fs::write(path, b"modified").is_ok());
        checkpoint(ProbeCheckpoint::RuntimeWrite)?;
        results.insert(
            "runtime_write",
            fs::write(
                std::env::current_exe()?
                    .parent()
                    .ok_or("no parent")?
                    .join("runtime.txt"),
                b"modified",
            )
            .is_ok(),
        );
        checkpoint(ProbeCheckpoint::ScratchWrite)?;
        results.insert(
            "scratch_write",
            fs::write("allowed.txt", b"allowed").is_ok(),
        );
        checkpoint(ProbeCheckpoint::CallerEnvironment)?;
        results.insert(
            "caller_environment",
            std::env::var_os("EW_SYNTHETIC_CALLER_SECRET").is_some(),
        );
        checkpoint(ProbeCheckpoint::TcpConnect)?;
        results.insert(
            "direct_tcp",
            TcpStream::connect_timeout(&input.tcp.parse()?, Duration::from_secs(1)).is_ok(),
        );
        checkpoint(ProbeCheckpoint::HttpConnect)?;
        let http_connected = if let Ok(mut stream) =
            TcpStream::connect_timeout(&input.tcp.parse()?, Duration::from_secs(1))
        {
            let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
            let _ = stream.write_all(b"GET /synthetic HTTP/1.0\r\n\r\n");
            // A connection alone is a violation, irrespective of the reply.
            true
        } else {
            false
        };
        results.insert("direct_http", http_connected);
        checkpoint(ProbeCheckpoint::UdpProbe)?;
        let udp = exchange(input.dns.parse()?, &input.udp_marker)?;
        results.insert("direct_udp_send", udp.send_accepted);
        results.insert("direct_udp_reply", udp.reply_received);
        checkpoint(ProbeCheckpoint::ChildSpawn)?;
        let spawned = match std::process::Command::new(std::env::current_exe()?)
            .arg("--grandchild")
            .spawn()
        {
            Ok(mut process) => {
                let _ = process.kill();
                let _ = process.wait();
                true
            }
            Err(_) => false,
        };
        // Creation is a violation even if the child immediately exits nonzero.
        results.insert("child_process", spawned);
        checkpoint(ProbeCheckpoint::ResultWrite)?;
        fs::write("result.json", serde_json::to_vec(&results)?)?;
        checkpoint(ProbeCheckpoint::Completed)?;
        Ok(())
    }
    fn utf16(value: &std::ffi::OsStr) -> Vec<u16> {
        value.encode_wide().chain(Some(0)).collect()
    }
    /// Deliberately unconstrained control, only for owned synthetic sentinels.
    /// TRUE handle inheritance proves the hostile handle attempt is meaningful.
    fn baseline(executable: &Path, input: &Path, scratch: &Path) -> AnyResult<()> {
        let name = utf16(executable.as_os_str());
        let directory = utf16(scratch.as_os_str());
        let mut command = utf16(std::ffi::OsStr::new(&format!(
            "\"{}\" --child \"{}\"",
            executable.display(),
            input.display()
        )));
        let mut startup: STARTUPINFOW = unsafe { zeroed() };
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        let mut process: PROCESS_INFORMATION = unsafe { zeroed() };
        if unsafe {
            CreateProcessW(
                name.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_NO_WINDOW,
                null(),
                directory.as_ptr(),
                &startup,
                &mut process,
            )
        } == 0
        {
            return Err("synthetic baseline did not launch".into());
        }
        let status = unsafe { WaitForSingleObject(process.hProcess, 15000) };
        let mut code = 1;
        unsafe {
            if status != WAIT_OBJECT_0 {
                TerminateProcess(process.hProcess, 1);
                WaitForSingleObject(process.hProcess, 5000);
            }
            GetExitCodeProcess(process.hProcess, &mut code);
            CloseHandle(process.hThread);
            CloseHandle(process.hProcess);
        }
        if status != WAIT_OBJECT_0 || code != 0 {
            return Err("synthetic baseline failed".into());
        }
        Ok(())
    }
    fn require(value: bool, message: &'static str) -> AnyResult<()> {
        if value {
            Ok(())
        } else {
            Err(message.into())
        }
    }
    fn execute(
        java: Option<&Path>,
        report: &mut BTreeMap<String, serde_json::Value>,
    ) -> AnyResult<()> {
        report.insert("phase".into(), serde_json::json!("setup"));
        let temporary = tempfile::Builder::new()
            .prefix("ew-windows-controls-")
            .tempdir()?;
        let root = temporary.path();
        let runtime = root.join("runtime");
        let jobs = root.join("jobs");
        let controls = root.join("controls");
        for directory in [&runtime, &jobs, &controls] {
            fs::create_dir(directory)?;
        }
        let executable = runtime.join("probe.exe");
        fs::copy(std::env::current_exe()?, &executable)?;
        fs::write(runtime.join("runtime.txt"), b"retained runtime")?;
        let other = root.join("other-workspace.txt");
        fs::write(&other, b"private synthetic evidence")?;
        let original = root.join("original.txt");
        fs::write(&original, b"retained original")?;
        let secret = root.join("handle-secret.txt");
        fs::write(&secret, SECRET)?;
        protect_private_tree(root)?;
        let mut handle = File::open(&secret)?;
        require(
            unsafe {
                SetHandleInformation(
                    handle.as_raw_handle(),
                    HANDLE_FLAG_INHERIT,
                    HANDLE_FLAG_INHERIT,
                )
            } != 0,
            "cannot prepare inherited-handle control",
        )?;
        let tcp = TcpListener::bind("127.0.0.1:0")?;
        tcp.set_nonblocking(true)?;
        let markers = Markers::new();
        let mut udp_listener = Some(Listener::start(markers.clone())?);
        let dns_address = udp_listener
            .as_ref()
            .ok_or("UDP listener missing")?
            .address
            .to_string();
        let tcp_address = tcp.local_addr()?.to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let tcp_stop = stop.clone();
        let tcp_thread = std::thread::spawn(move || {
            while !tcp_stop.load(Ordering::Relaxed) {
                if let Ok((mut connection, _)) = tcp.accept() {
                    let _ = connection.set_read_timeout(Some(Duration::from_millis(250)));
                    let mut bytes = [0; 128];
                    let _ = connection.read(&mut bytes);
                    let _ = connection.write_all(b"HTTP/1.0 200 OK\r\n\r\nsynthetic");
                } else {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        });
        let tests = (|| -> AnyResult<()> {
            let mut input = Input {
                mode: "probe".into(),
                other,
                original: original.clone(),
                tcp: tcp_address,
                dns: dns_address,
                udp_marker: markers.before.clone(),
                handle: handle.as_raw_handle() as usize,
            };
            let input_path = controls.join("input.json");
            fs::write(&input_path, serde_json::to_vec(&input)?)?;
            std::env::set_var("EW_SYNTHETIC_CALLER_SECRET", "synthetic");
            report.insert("phase".into(), serde_json::json!("unconfined_control"));
            baseline(&executable, &input_path, &controls)?;
            let baseline_checkpoint: ProbeCheckpoint =
                serde_json::from_slice(&fs::read(controls.join("probe-checkpoint.json"))?)?;
            report.insert(
                "baseline_checkpoint".into(),
                serde_json::to_value(baseline_checkpoint)?,
            );
            require(
                baseline_checkpoint == ProbeCheckpoint::Completed,
                "baseline checkpoints incomplete",
            )?;
            let mut baseline_results: BTreeMap<String, bool> =
                serde_json::from_slice(&fs::read(controls.join("result.json"))?)?;
            require(
                baseline_results.len() == 17 && !baseline_results.contains_key("inherited_handle"),
                "main baseline permission count mismatch",
            )?;
            report.insert(
                "phase".into(),
                serde_json::json!("unconfined_handle_control"),
            );
            input.mode = "inherited-handle".into();
            fs::write(&input_path, serde_json::to_vec(&input)?)?;
            handle.seek(SeekFrom::Start(0))?;
            baseline(&executable, &input_path, &controls)?;
            let handle_control: HandleObservation =
                serde_json::from_slice(&fs::read(controls.join("result.json"))?)?;
            let handle_checkpoint: ProbeCheckpoint =
                serde_json::from_slice(&fs::read(controls.join("probe-checkpoint.json"))?)?;
            require(
                !handle_control.app_container
                    && handle_control.sentinel_read
                    && handle_checkpoint == ProbeCheckpoint::Completed,
                "inherited-handle positive control failed",
            )?;
            report.insert(
                "baseline_handle".into(),
                serde_json::to_value(&handle_control)?,
            );
            baseline_results.insert("inherited_handle".into(), handle_control.sentinel_read);
            report.insert("baseline".into(), serde_json::to_value(&baseline_results)?);
            require(
                baseline_results
                    .iter()
                    .all(|(key, value)| *value == (key != "app_container"))
                    && baseline_results.len() == 18,
                "unconfined control did not establish every attempted permission",
            )?;
            fs::write(&original, b"retained original")?;
            fs::write(runtime.join("runtime.txt"), b"retained runtime")?;
            input.mode = "probe".into();
            input.udp_marker = markers.confined.clone();
            let mut request = Request {
                runtime,
                executable: "probe.exe".into(),
                arguments: vec!["--child".into(), "$EW_INPUT".into()],
                input: serde_json::to_vec(&input)?,
                scratch_parent: jobs.clone(),
                wall_time: Duration::from_secs(15),
                memory_bytes: 512 * 1024 * 1024,
            };
            report.insert("phase".into(), serde_json::json!("confined_permissions"));
            let mut diagnostics = ProbeDiagnostics::default();
            let output = run_probe(&request, || false, &mut diagnostics);
            report.insert(
                "confined_diagnostics".into(),
                serde_json::to_value(&diagnostics)?,
            );
            let output = output?;
            let mut confined: BTreeMap<String, bool> = serde_json::from_slice(&output.bytes)?;
            report.insert(
                "confined_permissions".into(),
                serde_json::to_value(&confined)?,
            );
            // Keep send acceptance as raw evidence, not a delivery predicate.
            // Winsock explicitly does not promise delivery after sendto success.
            let before_udp = UdpObservation {
                send_accepted: baseline_results["direct_udp_send"],
                reply_received: baseline_results["direct_udp_reply"],
            };
            let confined_udp = UdpObservation {
                send_accepted: *confined
                    .get("direct_udp_send")
                    .ok_or("UDP send observation missing")?,
                reply_received: *confined
                    .get("direct_udp_reply")
                    .ok_or("UDP reply observation missing")?,
            };
            report.insert("phase".into(), serde_json::json!("udp_delivery_controls"));
            report.insert(
                "udp_delivery_rule".into(),
                serde_json::json!("loopback_delivery_v1"),
            );
            input.mode = "udp-control".into();
            input.udp_marker = markers.after.clone();
            fs::write(&input_path, serde_json::to_vec(&input)?)?;
            baseline(&executable, &input_path, &controls)?;
            let after_udp: UdpObservation =
                serde_json::from_slice(&fs::read(controls.join("result.json"))?)?;
            let after_checkpoint: ProbeCheckpoint =
                serde_json::from_slice(&fs::read(controls.join("probe-checkpoint.json"))?)?;
            report.insert("udp_after_control".into(), serde_json::to_value(after_udp)?);
            report.insert(
                "udp_after_checkpoint".into(),
                serde_json::to_value(after_checkpoint)?,
            );
            let counts = udp_listener
                .take()
                .ok_or("UDP listener missing")?
                .finish()?;
            report.insert("udp_delivery_counts".into(), serde_json::to_value(counts)?);
            report.insert(
                "confined_udp_received".into(),
                serde_json::json!(counts.confined),
            );
            accept_delivery_denial(
                before_udp,
                confined_udp,
                after_udp,
                diagnostics.last_worker_checkpoint == Some(ProbeCheckpoint::Completed)
                    && after_checkpoint == ProbeCheckpoint::Completed,
                counts,
            )?;
            let expected: BTreeMap<String, bool> = baseline_results
                .keys()
                .filter(|key| key.as_str() != "direct_udp_send")
                .map(|key| {
                    (
                        key.clone(),
                        matches!(
                            key.as_str(),
                            "app_container" | "input_read" | "scratch_write"
                        ),
                    )
                })
                .chain([("direct_udp_delivered".into(), false)])
                .collect();
            confined.remove("direct_udp_send");
            confined.insert("direct_udp_delivered".into(), counts.confined != 0);
            let main_expected: BTreeMap<_, _> = expected
                .iter()
                .filter(|(key, _)| key.as_str() != "inherited_handle")
                .map(|(key, value)| (key.clone(), *value))
                .collect();
            let permissions_completed = confined == main_expected;
            require(
                permissions_completed,
                "AppContainer violated a synthetic permission boundary",
            )?;
            report.insert(
                "phase".into(),
                serde_json::json!("confined_inherited_handle"),
            );
            input.mode = "inherited-handle".into();
            request.input = serde_json::to_vec(&input)?;
            handle.seek(SeekFrom::Start(0))?;
            let mut handle_diagnostics = ProbeDiagnostics::default();
            let handle_result = run_probe(&request, || false, &mut handle_diagnostics);
            report.insert(
                "handle_diagnostics".into(),
                serde_json::to_value(&handle_diagnostics)?,
            );
            report.insert(
                "handle_exit".into(),
                serde_json::json!(handle_result.as_ref().err().map(ToString::to_string)),
            );
            let handle_outcome = accept_handle_outcome(
                handle_control.sentinel_read,
                permissions_completed,
                handle_result,
                &handle_diagnostics,
            )?;
            require(
                fs::read_dir(&jobs)?.next().is_none(),
                "scratch survived isolated handle probe",
            )?;
            report.insert(
                "confined_handle".into(),
                serde_json::to_value(handle_outcome)?,
            );
            confined.insert("inherited_handle".into(), false);
            report.insert("confined".into(), serde_json::to_value(&confined)?);
            require(
                confined == expected,
                "combined permission observations are incomplete",
            )?;
            require(
                fs::read(&original)? == b"retained original",
                "original sentinel changed",
            )?;
            for mode in [
                "timeout",
                "disk",
                "memory",
                "handles",
                "cancel",
                "output-large",
                "output-hardlink",
                "restricted-directory",
                "named-stream",
            ] {
                report.insert("phase".into(), serde_json::json!(mode));
                input.mode = if mode == "cancel" { "timeout" } else { mode }.into();
                request.input = serde_json::to_vec(&input)?;
                request.wall_time = Duration::from_secs(3);
                let start = Instant::now();
                let mut diagnostics = ProbeDiagnostics::default();
                let result = run_probe(
                    &request,
                    || mode == "cancel" && start.elapsed() >= Duration::from_millis(750),
                    &mut diagnostics,
                );
                let error = result.err().ok_or("hostile mode unexpectedly succeeded")?;
                let correct = matches!(
                    (mode, &error),
                    ("timeout", Error::Blocked("worker wall-time exceeded"))
                        | ("disk", Error::Blocked("tree disk budget exceeded"))
                        | ("handles", Error::Blocked("worker handle budget exceeded"))
                        | ("cancel", Error::Blocked("cancelled; worker job terminated"))
                        | ("output-large", Error::Blocked("result exceeds bound"))
                        | ("output-hardlink", Error::Blocked("invalid output file"))
                        | ("memory", Error::Exit(_))
                        | ("named-stream", Error::Blocked("named data stream rejected"))
                        | (
                            "restricted-directory",
                            Error::Io(std::io::ErrorKind::PermissionDenied)
                        )
                );
                report.insert(mode.into(), serde_json::json!({"rejected":true,"expected_failure":correct,"diagnostic":error.to_string(),"checkpoints":diagnostics}));
                require(correct, "hostile mode failed for an unexpected reason")?;
                require(
                    start.elapsed() < Duration::from_secs(15),
                    "bounded probe exceeded harness wall time",
                )?;
                require(
                    fs::read_dir(&jobs)?.next().is_none(),
                    "scratch survived a failed job",
                )?;
            }
            report.insert("phase".into(), serde_json::json!("readonly-file"));
            input.mode = "readonly-file".into();
            request.input = serde_json::to_vec(&input)?;
            match run(&request, || false) {
                Ok(output) => {
                    require(output.bytes == b"{}", "readonly worker result mismatch")?;
                    require(
                        fs::read_dir(&jobs)?.next().is_none(),
                        "successful readonly cleanup retained scratch",
                    )?;
                    report.insert(
                        "readonly-file".into(),
                        serde_json::json!({"cleanup":"removed","result_accepted":true}),
                    );
                }
                Err(Error::Cleanup { prior: None }) => {
                    require(
                        fs::read_dir(&jobs)?.next().is_some(),
                        "cleanup failure had no retained scratch",
                    )?;
                    report.insert("readonly-file".into(), serde_json::json!({"cleanup":"blocked_and_retained","result_accepted":false}));
                    // Explicit recovery of owned synthetic artifacts, never a
                    // production fallback or operation on real workspace data.
                    clear_synthetic_readonly(&jobs)?;
                    for entry in fs::read_dir(&jobs)? {
                        fs::remove_dir_all(entry?.path())?;
                    }
                }
                Err(error) => return Err(error.into()),
            }
            require(
                fs::read_dir(&jobs)?.next().is_none(),
                "readonly case left scratch after recovery",
            )?;
            if let Some(java_runtime) = java {
                report.insert("phase".into(), serde_json::json!("java_21"));
                // CI compiles this class before launch using its installed JDK;
                // the AppContainer receives real copied Java runtime bytes.
                let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/assets/JavaCompatibility.java");
                let stage = root.join("java-stage");
                fs::create_dir(&stage)?;
                copy_runtime(java_runtime, &stage)?;
                let status = std::process::Command::new(java_runtime.join("bin/javac.exe"))
                    .arg("--release")
                    .arg("21")
                    .arg("-d")
                    .arg(&stage)
                    .arg(source)
                    .status()?;
                require(
                    status.success(),
                    "Java compatibility probe compilation failed",
                )?;
                let java_request = Request {
                    runtime: stage,
                    executable: "bin/java.exe".into(),
                    arguments: vec![
                        "-Xmx64m".into(),
                        "-XX:-UsePerfData".into(),
                        "-XX:+DisableAttachMechanism".into(),
                        "-Djava.io.tmpdir=$EW_SCRATCH".into(),
                        "-cp".into(),
                        "$EW_RUNTIME".into(),
                        "JavaCompatibility".into(),
                        "$EW_INPUT".into(),
                    ],
                    input: b"synthetic Java input".to_vec(),
                    scratch_parent: jobs.clone(),
                    wall_time: Duration::from_secs(30),
                    memory_bytes: 512 * 1024 * 1024,
                };
                let output = run(&java_request, || false)?;
                let result: serde_json::Value = serde_json::from_slice(&output.bytes)?;
                require(
                    result == serde_json::json!({"java_input":true,"java_output":true}),
                    "Java assigned I/O did not work",
                )?;
                report.insert("java_21".into(), result);
            } else {
                report.insert("java_21".into(), serde_json::json!({"state":"not_run"}));
            }
            require(
                fs::read_dir(&jobs)?.next().is_none(),
                "scratch survived success",
            )?;
            Ok(())
        })();
        std::env::remove_var("EW_SYNTHETIC_CALLER_SECRET");
        stop.store(true, Ordering::Relaxed);
        let tcp_joined = tcp_thread.join().is_ok();
        // Early failures still stop/join the listener and retain observed
        // counts; incomplete controls can never produce a successful result.
        if let Some(listener) = udp_listener.take() {
            let counts = listener.finish()?;
            report.insert("udp_delivery_counts".into(), serde_json::to_value(counts)?);
            report.insert(
                "confined_udp_received".into(),
                serde_json::json!(counts.confined),
            );
        }
        require(tcp_joined, "synthetic TCP listener failed")?;
        tests?;
        temporary.close()?;
        report.insert("phase".into(), serde_json::json!("complete"));
        Ok(())
    }
    fn clear_synthetic_readonly(path: &Path) -> AnyResult<()> {
        use std::os::windows::fs::MetadataExt;
        let metadata = fs::symlink_metadata(path)?;
        require(
            metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
            "synthetic recovery refuses reparse points",
        )?;
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                clear_synthetic_readonly(&entry?.path())?;
            }
        } else if metadata.permissions().readonly() {
            let name = utf16(path.as_os_str());
            require(
                unsafe {
                    SetFileAttributesW(
                        name.as_ptr(),
                        metadata.file_attributes() & !FILE_ATTRIBUTE_READONLY,
                    )
                } != 0,
                "synthetic readonly reset failed",
            )?;
        }
        Ok(())
    }
    fn copy_runtime(source: &Path, destination: &Path) -> AnyResult<()> {
        use std::os::windows::fs::MetadataExt;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            require(
                metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
                "Java stage contains a reparse point",
            )?;
            let target = destination.join(entry.file_name());
            if metadata.is_dir() {
                fs::create_dir(&target)?;
                copy_runtime(&entry.path(), &target)?;
            } else {
                fs::copy(entry.path(), target)?;
            }
        }
        Ok(())
    }
    fn safe_error(error: &(dyn std::error::Error + 'static)) -> String {
        if let Some(error) = error.downcast_ref::<Error>() {
            return error.to_string();
        }
        if let Some(error) = error.downcast_ref::<std::io::Error>() {
            return format!("synthetic I/O error: {:?}", error.kind());
        }
        // Unknown errors are not stringified into a public artifact. The
        // explicit phase and retained test observations identify the failure.
        "synthetic control or result validation failed".into()
    }
    pub fn main() -> AnyResult<()> {
        let args: Vec<_> = std::env::args_os().collect();
        if args.get(1).is_some_and(|x| x == "--grandchild") {
            return Ok(());
        }
        if args.get(1).is_some_and(|x| x == "--child") {
            return child(Path::new(args.get(2).ok_or("child input missing")?));
        }
        if args.len() < 3 || args[1] != "--report" {
            return Err("usage: ew-windows-probe --report FILE [JAVA_HOME]".into());
        }
        let mut tests = BTreeMap::new();
        let result = execute(args.get(3).map(Path::new), &mut tests);
        let report = serde_json::json!({"schema_version":1,"scope":"native development AppContainer probe; not Windows 11 clean installation","complete_release":false,"passed":result.is_ok(),"tests":tests,"failure":result.as_ref().err().map(|error|safe_error(error.as_ref())),"unverified":["Windows 11 clean installed artifact","Python/OCR/Chromium compatibility","hard disk and handle quotas","non-loopback and Windows DNS resolver paths","signed installer","independent security acceptance"]});
        let path = Path::new(&args[2]);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec_pretty(&report)?)?;
        result
    }
}
#[cfg(windows)]
fn main() {
    if native::main().is_err() {
        eprintln!("Windows native probe failed; no release claim was produced");
        std::process::exit(1);
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows native probe requires Windows; no fallback exists");
    std::process::exit(1);
}
