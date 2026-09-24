//! Native synthetic checks only. No application workflow calls this harness.
use super::*;
use serde_json::{json, Value};
use std::{
    fs,
    net::TcpListener,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};
type Report = BTreeMap<String, Value>;

struct SyntheticEnvironment;
impl SyntheticEnvironment {
    fn set() -> Result<Self> {
        bounded(
            std::env::var_os("EW_SYNTHETIC_JAVA_SECRET").is_none(),
            "synthetic caller variable already exists",
        )?;
        std::env::set_var("EW_SYNTHETIC_JAVA_SECRET", "synthetic-only");
        Ok(Self)
    }
}
impl Drop for SyntheticEnvironment {
    fn drop(&mut self) {
        std::env::remove_var("EW_SYNTHETIC_JAVA_SECRET");
    }
}
fn phase(report: &mut Report, name: &str) {
    report.insert("phase".into(), json!(name));
}
fn empty(path: &Path) -> Result<()> {
    bounded(
        fs::read_dir(path)?.next().is_none(),
        "Java job scratch survived cleanup",
    )
}
fn execute_recorded(
    root: &Path,
    jobs: &Path,
    job: &Job,
    report: &mut Report,
) -> Result<JavaOutput> {
    let mut diagnostics = diagnostics::FailureDiagnostics::default();
    let outcome = execute_diagnosed(root, jobs, job, || false, Some(&mut diagnostics));
    if outcome.is_err() {
        report.insert("failed_worker_diagnostics".into(), json!(diagnostics));
    }
    outcome
}
fn control_recorded(
    parser: &Path,
    controls: &Path,
    document: crate::windows::ControlDocument,
    report: &mut Report,
) -> Result<JavaOutput> {
    let mut diagnostics = diagnostics::FailureDiagnostics::default();
    let label = match document {
        crate::windows::ControlDocument::Text => "file_worker_positive_control",
        crate::windows::ControlDocument::Pdf => "pdf_file_worker_positive_control",
        crate::windows::ControlDocument::FontCorpus => "font_corpus_positive_control",
        crate::windows::ControlDocument::EmbeddedFont => "embedded_font_positive_control",
        crate::windows::ControlDocument::Index => "index_positive_control",
    };
    let control = crate::windows::file_worker_control(parser, controls, document, &mut diagnostics);
    report.insert(
        label.into(),
        json!({
            "passed":control.is_ok(), "failure":control.as_ref().err().map(ToString::to_string),
            "output_sha256":control.as_ref().ok().map(|output| &output.output_sha256),
            "diagnostics":diagnostics,
            "scope":"synthetic FileWorker, no AppContainer; unchanged recipe and resource bounds"
        }),
    );
    control
}
fn absolute(root: &Path) -> Result<std::path::PathBuf> {
    bounded(
        !root
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir)),
        "probe root traversal rejected",
    )?;
    Ok(if root.is_absolute() {
        root.to_owned()
    } else {
        std::env::current_dir()?.join(root)
    })
}
fn fixture(name: &str) -> &'static [u8] {
    match name {
        "notice.txt" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/parser/notice.txt"
        )),
        "font-corpus.pdf" => font_fixtures::FontFixture::Corpus.bytes(),
        "embedded-font.pdf" => font_fixtures::FontFixture::Embedded.bytes(),
        "notice.pdf" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/parser/notice.pdf"
        )),
        "notice.docx" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/parser/notice.docx"
        )),
        "no-text.pdf" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/parser/no-text.pdf"
        )),
        "traversal.zip" => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/parser/traversal.zip"
        )),
        _ => b"\0\xff\x80synthetic unsupported",
    }
}
fn expected(role: Role, confined: bool) -> BTreeMap<String, bool> {
    let mut result = BTreeMap::new();
    for name in [
        "assigned_input_read",
        "assigned_request_read",
        "scratch_write",
    ] {
        result.insert(name.into(), true);
    }
    for name in [
        "assigned_input_write",
        "assigned_request_write",
        "runtime_write",
        "other_runtime_read",
        "other_workspace_read",
        "original_read",
        "original_write",
        "sibling_index_read",
        "direct_network",
        "child_process",
        "caller_environment",
    ] {
        result.insert(name.into(), !confined);
    }
    for name in ["parser_class", "tika_class"] {
        result.insert(name.into(), role == Role::Parser);
    }
    for name in [
        "search_class",
        "lucene_class",
        "memory_directory_class",
        "directory_policy_present",
        "directory_policy_exact",
        "assigned_index_read",
    ] {
        result.insert(name.into(), role == Role::Search);
    }
    result.insert(
        "assigned_index_write".into(),
        role == Role::Search && !confined,
    );
    result
}
fn probe_arguments(
    prepared: &mut Prepared<'_>,
    role: Role,
    other_runtime: &Path,
    private: &Path,
    port: u16,
    segment: &str,
) -> Result<()> {
    // Only this closed synthetic helper changes the reviewed recipe's entrypoint.
    // These paths refer to setup-owned disposable sentinels or staged runtime.
    let text = |path: &Path| {
        path.to_str()
            .map(str::to_owned)
            .ok_or(Error::Blocked("synthetic path encoding rejected"))
    };
    let entry = prepared
        .request
        .arguments
        .iter()
        .position(|v| v == "workbench.FileWorker")
        .ok_or(Error::Blocked("fixed entrypoint absent"))?;
    prepared.request.arguments.truncate(entry);
    prepared
        .request
        .arguments
        .insert(0, "-Dworkbench.assignedRequest=$EW_REQUEST".into());
    if role == Role::Search {
        prepared
            .request
            .arguments
            .insert(0, format!(r"-Dworkbench.probeIndex=$EW_INDEX\{segment}"));
    }
    prepared.request.arguments.extend([
        "workbench.WindowsJavaProbe".into(),
        if role == Role::Parser {
            "parser"
        } else {
            "search"
        }
        .into(),
        "$EW_RUNTIME".into(),
        text(&other_runtime.join("worker.jar"))?,
        text(&private.join("workspace.txt"))?,
        text(&private.join("original.txt"))?,
        text(&private.join("index").join(segment))?,
        port.to_string(),
    ]);
    Ok(())
}
fn baseline(prepared: &Prepared<'_>, parent: &Path) -> Result<BTreeMap<String, bool>> {
    let scratch = tempfile::Builder::new()
        .prefix("java-control-")
        .tempdir_in(parent)?;
    let root = scratch.path().canonicalize()?;
    let input = root.join("input.json");
    let request = root.join("request.json");
    let index = root.join("index");
    fs::write(&input, &prepared.request.input)?;
    fs::write(&request, &prepared.metadata)?;
    if let Some(snapshot) = prepared.snapshot() {
        fs::create_dir(&index)?;
        for file in &snapshot.files {
            fs::write(index.join(&file.name), &file.bytes)?;
        }
    }
    let replacements = [
        ("$EW_RUNTIME", prepared.request.runtime.as_path()),
        ("$EW_INPUT", input.as_path()),
        ("$EW_SCRATCH", root.as_path()),
        ("$EW_REQUEST", request.as_path()),
        ("$EW_INDEX", index.as_path()),
    ];
    let mut arguments = Vec::new();
    for source in &prepared.request.arguments {
        let mut value = source.clone();
        for (key, path) in replacements {
            value = value.replace(
                key,
                path.to_str()
                    .ok_or(Error::Blocked("synthetic control path encoding rejected"))?,
            );
        }
        arguments.push(value);
    }
    let mut child = Command::new(prepared.request.runtime.join(&prepared.request.executable))
        .args(arguments)
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("EW_SYNTHETIC_JAVA_SECRET", "synthetic-only")
        .spawn()?;
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            bounded(status.success(), "unconfined Java control failed")?;
            break;
        }
        if start.elapsed() > Duration::from_secs(30) {
            child.kill()?;
            child.wait()?;
            return Err(Error::Blocked("unconfined Java control timed out"));
        }
        thread::sleep(Duration::from_millis(20));
    }
    let bytes = runtime::read_bounded(&root.join("result.json"), 1024 * 1024)?;
    let result = decode(&bytes)?;
    scratch.close()?;
    Ok(result)
}

/// Explicit development command: only fresh staged runtimes and compiled
/// synthetic fixtures. It does not enable the application's Windows engines.
pub fn development_probe(staged: &Path, report: &mut BTreeMap<String, Value>) -> Result<()> {
    phase(report, "runtime_inventory");
    let staged = absolute(staged)?;
    let parser = staged.join("parser");
    let search = staged.join("search");
    runtime::verify(&parser, Role::Parser)?;
    runtime::verify(&search, Role::Search)?;
    let marker: Value = decode(&runtime::read_bounded(
        &staged.join("development-only.json"),
        4096,
    )?)?;
    bounded(
        marker == json!({"schema_version":1,"development_only":true,"roles":["parser","search"]}),
        "development staging marker rejected",
    )?;
    let entries = fs::read_dir(&staged)?
        .map(|e| e.map(|v| v.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    bounded(entries.len() == 3, "unexpected staged root assets")?;
    // These are generated staging copies, never the caller's installed JDK or
    // source JARs. Each actual worker receives a further isolated runtime copy.
    crate::protect_private_tree(&parser)?;
    crate::protect_private_tree(&search)?;
    let temporary = tempfile::Builder::new()
        .prefix("ew-java-probe-")
        .tempdir()?;
    let root = temporary.path().canonicalize()?;
    let jobs = root.join("jobs");
    let controls = root.join("controls");
    let private = root.join("private");
    fs::create_dir(&jobs)?;
    fs::create_dir(&controls)?;
    fs::create_dir(&private)?;
    fs::write(private.join("workspace.txt"), b"synthetic private sentinel")?;
    fs::write(private.join("original.txt"), fixture("notice.txt"))?;
    crate::protect_private_tree(&private)?;
    phase(report, "file_worker_positive_control");
    let control = control_recorded(
        &parser,
        &controls,
        crate::windows::ControlDocument::Text,
        report,
    );
    // Preserve a normal control failure alongside the first confined outcome.
    // An unacknowledged termination/cleanup failure stops all further launches.
    if matches!(&control, Err(Error::Cleanup { .. })) {
        return control.map(|_| ());
    }
    empty(&controls)?;
    let mut control_failure = control.err();
    let mut documents = Vec::new();
    for name in [
        "notice.txt",
        "notice.pdf",
        "font-corpus.pdf",
        "embedded-font.pdf",
        "notice.docx",
        "no-text.pdf",
        "traversal.zip",
        "unsupported.bin",
    ] {
        let pdf_control = match name {
            "notice.pdf" => Some(crate::windows::ControlDocument::Pdf),
            "font-corpus.pdf" => Some(crate::windows::ControlDocument::FontCorpus),
            "embedded-font.pdf" => Some(crate::windows::ControlDocument::EmbeddedFont),
            _ => None,
        };
        if let Some(document) = pdf_control {
            phase(report, &format!("control_{name}"));
            let control = control_recorded(&parser, &controls, document, report);
            if matches!(&control, Err(Error::Cleanup { .. })) {
                return control.map(|_| ());
            }
            empty(&controls)?;
            control_failure = control.err();
        }
        phase(report, &format!("parse_{name}"));
        let bytes = fixture(name);
        let job = Job::parse(bytes.to_vec())?;
        let outcome = execute_recorded(&parser, &jobs, &job, report);
        if name == "notice.txt" || pdf_control.is_some() {
            report.insert(
                match name {
                    "notice.txt" => "first_confined_parse",
                    "notice.pdf" => "confined_pdf_parse",
                    "font-corpus.pdf" => "confined_font_corpus",
                    _ => "confined_embedded_font",
                }
                .into(),
                json!({
                    "passed":outcome.is_ok(),
                    "failure":outcome.as_ref().err().map(ToString::to_string),
                    "output_sha256":outcome.as_ref().ok().map(|output| &output.output_sha256)
                }),
            );
        }
        let output = outcome?;
        if let Some(failure) = control_failure.take() {
            return Err(failure);
        }
        let result: Value = decode(&output.bytes)?;
        match name {
            "notice.txt" | "notice.pdf" | "notice.docx" => {
                bounded(
                    result["status"]
                        == if name == "notice.txt" {
                            "complete"
                        } else {
                            "partial"
                        }
                        && result["text"].as_str().is_some_and(|text| {
                            text.contains("Rowan Ellis")
                                && text.contains("Fictional Harbour Cooperative")
                        }),
                    "real parser text/status mismatch",
                )?;
                documents.push(Document {
                    id: name.into(),
                    name: name.into(),
                    text: result["text"].as_str().unwrap().into(),
                });
            }
            "font-corpus.pdf" => font_fixtures::FontFixture::Corpus.validate(&result)?,
            "embedded-font.pdf" => font_fixtures::FontFixture::Embedded.validate(&result)?,
            "no-text.pdf" => font_fixtures::validate_no_text_pdf(&result)?,
            "traversal.zip" => bounded(
                result["status"] == "failed" && result["error"] == "archive_limits",
                "archive traversal was not rejected",
            )?,
            _ => bounded(
                result["status"] == "unsupported",
                "unsupported input was not explicit",
            )?,
        }
        if name.ends_with(".pdf") {
            bounded(
                result["parser"] == font_fixtures::PARSER,
                "PDF font policy identity absent",
            )?;
        }
        if name == "notice.pdf" {
            bounded(
                result["limitations"].as_array().is_some_and(|v| {
                    v.iter().any(|x| x == "font_substituted")
                        && v.iter().any(|x| x == "font_coverage_unverified")
                }),
                "PDF font fallback limitations absent",
            )?;
        }
        report.insert(format!("parse_{name}"),json!({"source_sha256":format!("{:x}",Sha256::digest(bytes)),"output_sha256":output.output_sha256,
            "source_bytes":bytes.len(),"status":result["status"],"parser":result["parser"],"limitations":result["limitations"],"text_bytes":result["text"].as_str().unwrap_or("").len()}));
        empty(&jobs)?;
    }
    phase(report, "index_positive_control");
    let control = control_recorded(
        &search,
        &controls,
        crate::windows::ControlDocument::Index,
        report,
    );
    if matches!(&control, Err(Error::Cleanup { .. })) {
        return control.map(|_| ());
    }
    empty(&controls)?;
    phase(report, "lucene_index");
    let indexed = execute_recorded(&search, &jobs, &Job::index(7, documents)?, report);
    report.insert(
        "confined_index".into(),
        json!({
            "passed":indexed.is_ok(),"failure":indexed.as_ref().err().map(ToString::to_string),
            "output_sha256":indexed.as_ref().ok().map(|output| &output.output_sha256)
        }),
    );
    let indexed = indexed?;
    control?;
    let snapshot = indexed
        .index
        .ok_or(Error::Blocked("index snapshot missing"))?;
    report.insert("lucene_index".into(),json!({"directory_policy":snapshot.directory_policy(),"revision":snapshot.revision(),"files":snapshot.file_count(),"bytes":snapshot.total_bytes(),"output_sha256":indexed.output_sha256}));
    empty(&jobs)?;
    for (label, query, total) in [
        ("boolean", "Rowan AND Harbour", 3),
        ("phrase", "\"Rowan Ellis\"", 3),
        ("proximity", "\"Rowan Harbour\"~8", 3),
        ("fuzzy", "Rowen~1", 3),
        ("fielded", "name:notice.txt", 1),
        ("empty", "unrelatedzephyr", 0),
    ] {
        phase(report, &format!("lucene_{label}"));
        let output = execute_recorded(&search, &jobs, &Job::search(&snapshot, query)?, report)?;
        let result: Value = decode(&output.bytes)?;
        bounded(
            result["total"] == total
                && result["hits"]
                    .as_array()
                    .is_some_and(|h| h.len() == total as usize),
            "real Lucene query result mismatch",
        )?;
        report.insert(
            format!("lucene_{label}"),
            json!({"revision":7,"hits":total,"output_sha256":output.output_sha256}),
        );
        empty(&jobs)?;
    }
    // The denied sibling is a real captured Lucene snapshot, not a substitute
    // sentinel. The other private source is the actual compiled parser fixture.
    fs::create_dir(private.join("index"))?;
    for file in &snapshot.files {
        fs::write(private.join("index").join(&file.name), &file.bytes)?;
    }
    crate::protect_private_tree(&private)?;
    phase(report, "java_permission_controls");
    let _synthetic_environment = SyntheticEnvironment::set()?;
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = stopped.clone();
    let network = thread::spawn(move || -> std::io::Result<()> {
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((socket, _)) => drop(socket),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    });
    let permissions = (|| -> Result<()> {
        let segment = &snapshot
            .files
            .iter()
            .find(|file| file.name.starts_with("segments_"))
            .ok_or(Error::Blocked("index commit absent"))?
            .name;
        for role in [Role::Parser, Role::Search] {
            let label = if role == Role::Parser {
                "parser_boundary"
            } else {
                "search_boundary"
            };
            phase(report, label);
            let job = if role == Role::Parser {
                Job::parse(b"synthetic Java boundary input".to_vec())?
            } else {
                Job::search(&snapshot, "Rowan")?
            };
            let (runtime, other) = if role == Role::Parser {
                (&parser, &search)
            } else {
                (&search, &parser)
            };
            let mut prepared = prepare(runtime, &jobs, &job)?;
            probe_arguments(&mut prepared, role, other, &private, port, segment)?;
            let before = baseline(&prepared, &controls)?;
            report.insert(format!("{label}_before"), json!(before));
            bounded(
                before == expected(role, false),
                "Java boundary positive control failed",
            )?;
            let mut diagnostics = diagnostics::FailureDiagnostics::default();
            let output = crate::windows::run_java_probe(&prepared, &mut diagnostics);
            if output.is_err() {
                report.insert("failed_worker_diagnostics".into(), json!(diagnostics));
            }
            let output = output?;
            let confined: BTreeMap<String, bool> = decode(&output.bytes)?;
            report.insert(format!("{label}_confined"), json!(confined));
            bounded(
                confined == expected(role, true),
                "Java boundary predicates mismatched",
            )?;
            empty(&jobs)?;
            let after = baseline(&prepared, &controls)?;
            report.insert(format!("{label}_after"), json!(after));
            bounded(
                after == expected(role, false),
                "Java boundary after-control failed",
            )?;
        }
        Ok(())
    })();
    stopped.store(true, Ordering::Relaxed);
    let network = network
        .join()
        .map_err(|_| Error::Blocked("Java control listener panicked"))?;
    network?;
    permissions?;
    empty(&jobs)?;
    empty(&controls)?;
    temporary.close()?;
    phase(report, "complete");
    Ok(())
}
