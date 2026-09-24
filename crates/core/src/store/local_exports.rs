//! One session owns one private stage. Lock order: export registry, then workspace.
//! Files publish before a receipt exists. This is not a canonical DB transaction or backup record.
use super::*;
use crate::local_export::*;
use crate::transaction_export::MAX_EXPORT_JSON_BYTES;
use std::{
    collections::VecDeque,
    io::Read,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
const TTL: Duration = Duration::from_secs(120);
const COMPLETED_LIMIT: usize = 8;
struct Stage {
    prepared: PreparedExport,
    expires: Instant,
    links: u64,
}
#[derive(Default)]
struct Registry {
    stage: Option<Stage>,
    completed: VecDeque<SavedExportReceipt>,
    cleanup_error: bool,
}
struct Session {
    root: PathBuf,
    registry: Mutex<Registry>,
    stopping: AtomicBool,
    wake: Condvar,
}
pub(crate) struct NativeExports {
    session: Arc<Session>,
    janitor: Mutex<Option<JoinHandle<()>>>,
}
impl Workspace {
    pub(crate) fn start_native_exports(&self) -> Result<NativeExports> {
        NativeExports::start(self.root.clone())
    }
    pub(crate) fn native_export_content(
        &self,
        request: NativeExportRequest,
    ) -> Result<(ExportArtifact, String)> {
        match request {
            NativeExportRequest::Transactions {
                request,
                expected_revision,
                expected_row_count,
                expected_matching,
            } => {
                let export = self.export_transactions(&request, expected_revision)?;
                require(
                    export.row_count == expected_row_count && export.matching == expected_matching,
                    "Export count or matching profile changed; refresh the ledger",
                )?;
                Ok((
                    ExportArtifact::Transactions {
                        workspace_revision: export.workspace_revision,
                        request: export.request,
                        matching: export.matching,
                        query_sha256: export.query_sha256,
                        row_count: export.row_count,
                        bytes: export.bytes,
                        sha256: export.sha256,
                    },
                    export.json,
                ))
            }
            NativeExportRequest::HtmlReport {
                report_id,
                expected_sha256,
            } => {
                let report = self.inspect_report_snapshot(&report_id, &expected_sha256)?;
                Ok((
                    ExportArtifact::HtmlReport {
                        report_id: report.id,
                        workspace_revision: report.workspace_revision,
                        bytes: report.html.len() as u64,
                        sha256: report.sha256,
                    },
                    report.html,
                ))
            }
        }
    }
}
impl NativeExports {
    fn start(root: PathBuf) -> Result<Self> {
        let session = Arc::new(Session {
            root,
            registry: Mutex::new(Registry::default()),
            stopping: AtomicBool::new(false),
            wake: Condvar::new(),
        });
        session.directories()?;
        // Only this session's dedicated staging directory is inspected, never exports or originals.
        let mut count = 0;
        for entry in fs::read_dir(session.staging())? {
            count += 1;
            require(
                count <= 32,
                "Native export staging recovery exceeds its entry limit",
            )?;
            let entry = entry?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| Error::Cleanup("Unknown native export staging entry".into()))?;
            let key = name
                .strip_suffix(".pending")
                .ok_or_else(|| Error::Cleanup("Unknown native export staging entry".into()))?;
            valid_ticket(key)?;
            // A crash may leave the staged name linked to a published file. Only
            // unlink this known ordinary name; never modify its other links.
            ordinary(&entry.path(), None)?;
            fs::remove_file(entry.path())?;
        }
        let runner = session.clone();
        let janitor = thread::Builder::new()
            .name("native-export-expiry".into())
            .spawn(move || {
                let mut state = runner
                    .registry
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                loop {
                    if runner.stopping.load(Ordering::Acquire) {
                        return;
                    }
                    if runner.expire(&mut state).is_err() {
                        state.cleanup_error = true;
                    }
                    state = runner
                        .wake
                        .wait_timeout(state, Duration::from_secs(1))
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .0;
                }
            })?;
        Ok(Self {
            session,
            janitor: Mutex::new(Some(janitor)),
        })
    }
    pub(crate) fn prepare(
        &self,
        generate: impl FnOnce() -> Result<(ExportArtifact, String)>,
    ) -> Result<PreparedExport> {
        let mut state = self
            .session
            .registry
            .try_lock()
            .map_err(|_| Error::Blocked("A native export is already in progress".into()))?;
        self.session.available(&state)?;
        require(
            !state.cleanup_error,
            "Discard the retained stage after cleanup failure before preparing another",
        )?;
        self.session.expire(&mut state)?;
        require(
            state.stage.is_none(),
            "Commit or discard the prepared export before preparing another",
        )?;
        let (artifact, content) = generate()?;
        self.session.available(&state)?;
        require(
            content.len() <= MAX_EXPORT_JSON_BYTES
                && content.len() as u64 == artifact.bytes()
                && hash(content.as_bytes()) == artifact.sha256(),
            "Generated export failed its identity check",
        )?;
        self.session.directories()?;
        let prepared = PreparedExport {
            schema_version: 1,
            ticket: id(),
            expires_after_seconds: TTL.as_secs(),
            artifact,
        };
        let path = self.session.stage_path(&prepared.ticket);
        // Register before creating a file so every partial write is explicitly cleaned.
        state.stage = Some(Stage {
            prepared: prepared.clone(),
            expires: Instant::now() + TTL,
            links: 1,
        });
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            private_open(&mut options);
            let mut file = options.open(&path)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            drop(file);
            verify(&path, &prepared.artifact, 1)?;
            self.session.available(&state)?;
            Ok(prepared)
        })();
        if result.is_err() {
            self.session.cleanup(&mut state)?;
        } else if let Some(stage) = &mut state.stage {
            stage.expires = Instant::now() + TTL;
        }
        result
    }
    pub(crate) fn commit(
        &self,
        ticket: &str,
        sha256: &str,
        bytes: u64,
    ) -> Result<SavedExportReceipt> {
        valid_ticket(ticket)?;
        let mut state = self
            .session
            .registry
            .lock()
            .map_err(|_| Error::Blocked("Native export registry is unavailable".into()))?;
        self.session.available(&state)?;
        self.session.expire(&mut state)?;
        if let Some(receipt) = state.completed.iter().find(|value| value.ticket == ticket) {
            expected(&receipt.artifact, sha256, bytes)?;
            self.session.directories()?;
            verify(
                &self.session.output(&receipt.artifact),
                &receipt.artifact,
                1,
            )?;
            return Ok(receipt.clone());
        }
        let stage = state
            .stage
            .as_ref()
            .filter(|stage| stage.prepared.ticket == ticket)
            .ok_or_else(|| {
                Error::Validation("Export ticket is unknown or expired; prepare again".into())
            })?;
        let prepared = stage.prepared.clone();
        let links = stage.links;
        expected(&prepared.artifact, sha256, bytes)?;
        self.session.directories()?;
        let pending = self.session.stage_path(ticket);
        let target = self.session.output(&prepared.artifact);
        verify(&pending, &prepared.artifact, links)?;
        // Same-volume hard-link publication is atomic and cannot overwrite a prior export.
        let target_links = match fs::hard_link(&pending, &target) {
            Ok(()) => {
                if let Some(stage) = &mut state.stage {
                    stage.links = 2;
                }
                2
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => links,
            Err(error) => return Err(error.into()),
        };
        verify(&target, &prepared.artifact, target_links)?;
        if target_links == 2 {
            let staged = ordinary(&pending, Some(2))?;
            let published = ordinary(&target, Some(2))?;
            super::file_identity::unchanged(&staged, &published)?;
            #[cfg(windows)]
            {
                let mut options = OpenOptions::new();
                options.read(true);
                private_open(&mut options);
                let first = options.open(&pending)?;
                let second = options.open(&target)?;
                require(
                    super::file_identity::windows_handle(&first, 2)?
                        == super::file_identity::windows_handle(&second, 2)?,
                    "Published export is not the staged file",
                )?;
            }
        }
        sync_directory(&self.session.exports())?;
        self.session.cleanup(&mut state)?;
        verify(&target, &prepared.artifact, 1)?;
        let receipt = SavedExportReceipt {
            schema_version: 1,
            ticket: ticket.into(),
            filename: prepared.artifact.filename(),
            location: target.to_string_lossy().into_owned(),
            artifact: prepared.artifact,
        };
        state.completed.push_back(receipt.clone());
        while state.completed.len() > COMPLETED_LIMIT {
            state.completed.pop_front();
        }
        Ok(receipt)
    }
    pub(crate) fn discard(&self, ticket: &str) -> Result<DiscardedExport> {
        valid_ticket(ticket)?;
        let mut state = self
            .session
            .registry
            .lock()
            .map_err(|_| Error::Blocked("Native export registry is unavailable".into()))?;
        if let Some(receipt) = state.completed.iter().find(|value| value.ticket == ticket) {
            verify(
                &self.session.output(&receipt.artifact),
                &receipt.artifact,
                1,
            )?;
            return Ok(DiscardedExport::Saved {
                receipt: Box::new(receipt.clone()),
            });
        }
        if state
            .stage
            .as_ref()
            .is_some_and(|stage| stage.prepared.ticket == ticket)
        {
            self.session.cleanup(&mut state)?;
        }
        // Idempotent after expiry or a lost discard acknowledgement. Never touches another ticket.
        Ok(DiscardedExport::Discarded)
    }
    pub(crate) fn shutdown(&self) -> Result<()> {
        self.session.stopping.store(true, Ordering::Release);
        self.session.wake.notify_all();
        let mut janitor = self
            .janitor
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(thread) = janitor.take() {
            thread.join().map_err(|_| {
                Error::Cleanup("Native export expiry task did not stop normally".into())
            })?;
        }
        let mut state = self
            .session
            .registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.session.cleanup(&mut state)
    }
}
impl Drop for NativeExports {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
impl Session {
    fn staging(&self) -> PathBuf {
        self.root.join("native-export-staging")
    }
    fn exports(&self) -> PathBuf {
        self.root.join("exports")
    }
    fn stage_path(&self, ticket: &str) -> PathBuf {
        self.staging().join(format!("{ticket}.pending"))
    }
    fn output(&self, artifact: &ExportArtifact) -> PathBuf {
        self.exports().join(artifact.filename())
    }
    fn directories(&self) -> Result<()> {
        private_dir(&self.staging())?;
        private_dir(&self.exports())
    }
    fn available(&self, _state: &Registry) -> Result<()> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(Error::Blocked("Native export session is stopping".into()));
        }
        Ok(())
    }
    fn expire(&self, state: &mut Registry) -> Result<()> {
        if state
            .stage
            .as_ref()
            .is_some_and(|stage| Instant::now() >= stage.expires)
        {
            self.cleanup(state)?;
        }
        Ok(())
    }
    fn cleanup(&self, state: &mut Registry) -> Result<()> {
        if let Some(stage) = &state.stage {
            let path = self.stage_path(&stage.prepared.ticket);
            let result: Result<()> = (|| {
                reject_link_ancestors(&self.staging())?;
                match fs::symlink_metadata(&path) {
                    Ok(metadata) => {
                        // Unlink a substituted symlink itself; never follow it, chmod or recurse.
                        require(
                            !metadata.is_dir(),
                            "Native export staging entry became a directory",
                        )?;
                        fs::remove_file(&path)?;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
                require(
                    fs::symlink_metadata(&path)
                        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound),
                    "Native export stage still exists",
                )?;
                Ok(())
            })();
            if let Err(error) = result {
                state.cleanup_error = true;
                return Err(Error::Cleanup(format!(
                    "Native export staging cleanup failed: {error}"
                )));
            }
        }
        state.stage = None;
        state.cleanup_error = false;
        Ok(())
    }
}
fn valid_ticket(ticket: &str) -> Result<()> {
    require(
        ticket.len() == 36 && Uuid::parse_str(ticket).is_ok_and(|id| id.to_string() == ticket),
        "Invalid export ticket",
    )
}
fn expected(artifact: &ExportArtifact, sha256: &str, bytes: u64) -> Result<()> {
    require(
        artifact.sha256() == sha256 && artifact.bytes() == bytes,
        "Prepared export identity did not match",
    )
}
fn check_metadata(metadata: &fs::Metadata, links: Option<u64>) -> Result<()> {
    require(
        metadata.is_file() && !is_link(metadata) && metadata.len() <= MAX_EXPORT_JSON_BYTES as u64,
        "Export must be an ordinary bounded file",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        require(
            links.is_none_or(|expected| metadata.nlink() == expected),
            "Export has unexpected hard links",
        )?;
    }
    #[cfg(not(unix))]
    let _ = links;
    Ok(())
}
fn ordinary(path: &Path, links: Option<u64>) -> Result<fs::Metadata> {
    reject_link_ancestors(path)?;
    let metadata = fs::symlink_metadata(path)?;
    check_metadata(&metadata, links)?;
    Ok(metadata)
}
fn private_open(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000).share_mode(0x00000001);
    }
}
fn verify(path: &Path, artifact: &ExportArtifact, links: u64) -> Result<()> {
    verify_checked(path, artifact, links, |_| Ok(()), |_| Ok(()))
}
fn verify_checked(
    path: &Path,
    artifact: &ExportArtifact,
    links: u64,
    before_open: impl FnOnce(&Path) -> Result<()>,
    after_open: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let before = ordinary(path, Some(links))?;
    require(
        before.len() == artifact.bytes(),
        "Export byte length changed",
    )?;
    before_open(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    private_open(&mut options);
    let mut file = options.open(path)?;
    let opened = file.metadata()?;
    check_metadata(&opened, Some(links))?;
    super::file_identity::unchanged(&before, &opened)?;
    #[cfg(windows)]
    let identity = super::file_identity::windows_handle(&file, links)?;
    after_open(path)?;
    let mut digest = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        require(total <= artifact.bytes(), "Export grew during verification")?;
        digest.update(&buffer[..count]);
    }
    require(
        total == artifact.bytes() && format!("{:x}", digest.finalize()) == artifact.sha256(),
        "Export digest changed",
    )?;
    let finished = file.metadata()?;
    check_metadata(&finished, Some(links))?;
    super::file_identity::unchanged(&opened, &finished)?;
    let named = ordinary(path, Some(links))?;
    super::file_identity::unchanged(&opened, &named)?;
    #[cfg(windows)]
    {
        require(
            identity == super::file_identity::windows_handle(&file, links)?,
            "Export handle identity changed during read",
        )?;
        let named_handle = options.open(path)?;
        require(
            identity == super::file_identity::windows_handle(&named_handle, links)?,
            "Export named identity changed during read",
        )?;
    }
    Ok(())
}
fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
#[cfg(test)]
mod tests;
