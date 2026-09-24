use crate::{analytics, domain::*, policy, report, require, Error, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;
mod assessment;
mod citation_catalogue;
mod collection;
mod collection_jobs;
mod derivative_files;
mod desktop_summary;
mod docx_snapshots;
mod evidence;
use evidence::{all_evidence, find_evidence, get_evidence};
mod file_identity;
mod identity;
mod originals;
use originals::read_original;
#[cfg(test)]
mod evidence_identity_tests;
pub(crate) mod local_exports;
mod presentation;
mod processing;
mod processing_regions;
mod recovery;
mod report_snapshots;
mod review_decision_page;
mod statements;
mod transaction_analysis;
mod transaction_balance;
mod transaction_comparison;
mod transaction_export;
mod transaction_facets;
mod transaction_page;
mod transaction_search;
mod transaction_sources;
mod transfer_candidates;
#[cfg(test)]
mod view_tests;

const SCHEMA: u32 = 5;
// Only workspace refresh responses vary. Direct reader/job responses are unchanged.
#[derive(Clone, Copy)]
enum ResponseMode {
    Full,
    Presentation,
    Summary,
}
pub struct Workspace {
    root: PathBuf,
    conn: Connection,
    runtime: Option<crate::engines::Runtime>,
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT, including junctions.
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}
fn reject_link_ancestors(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => require(
                !is_link(&metadata),
                "Workspace path contains a link or reparse point",
            )?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
fn private_dir(path: &Path) -> Result<()> {
    reject_link_ancestors(path)?;
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn private_file(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}
fn all<T: DeserializeOwned>(conn: &Connection, kind: &str) -> Result<Vec<T>> {
    let mut stmt = conn.prepare("SELECT body FROM records WHERE kind=? ORDER BY sequence")?;
    let rows = stmt.query_map([kind], |row| row.get::<_, String>(0))?;
    rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
}
fn get<T: DeserializeOwned>(conn: &Connection, kind: &str, key: &str) -> Result<T> {
    let body: Option<String> = conn
        .query_row(
            "SELECT body FROM records WHERE kind=? AND id=?",
            params![kind, key],
            |r| r.get(0),
        )
        .optional()?;
    serde_json::from_str(
        &body.ok_or_else(|| Error::Validation(format!("Unknown {kind} identifier")))?,
    )
    .map_err(Into::into)
}
fn put<T: Serialize>(conn: &Connection, kind: &str, key: &str, value: &T) -> Result<()> {
    let revision: u64 = conn.query_row("SELECT revision FROM meta", [], |r| r.get(0))?;
    conn.execute("INSERT INTO history(kind,id,body,revision) SELECT kind,id,body,? FROM records WHERE kind=? AND id=?",params![revision+1,kind,key])?;
    conn.execute("INSERT INTO records(kind,id,body) VALUES(?,?,?) ON CONFLICT(kind,id) DO UPDATE SET body=excluded.body",params![kind,key,serde_json::to_string(value)?])?;
    Ok(())
}
fn reason(value: &str) -> Result<()> {
    require(
        !value.trim().is_empty() && value.len() <= 2000,
        "A review reason of 1 to 2000 bytes is required",
    )
}
fn record_decision(conn: &Connection, target: &str, state: ReviewState, why: &str) -> Result<()> {
    let d = ReviewDecision {
        id: id(),
        target_id: target.into(),
        state,
        reason: why.into(),
        at: now(),
    };
    put(conn, "decision", &d.id, &d)
}
impl Workspace {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        private_dir(&root)?;
        for directory in ["originals", "exports", "backups", "scratch"] {
            private_dir(&root.join(directory))?;
        }
        let db = root.join("workspace.db");
        if db.exists() {
            require(
                !fs::symlink_metadata(&db)?.file_type().is_symlink(),
                "Database must not be a symbolic link",
            )?;
        }
        let mut conn = Connection::open(&db)?;
        private_file(&db, 0o600)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA {
            return Err(Error::Blocked(
                "Workspace uses a newer schema; open it with a compatible application".into(),
            ));
        }
        if version == 0 {
            let tables:u32=conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",[],|r|r.get(0))?;
            require(
                tables == 0,
                "Unversioned nonempty database cannot be adopted",
            )?;
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch("CREATE TABLE meta(revision INTEGER NOT NULL); INSERT INTO meta VALUES(0);
                CREATE TABLE records(sequence INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, id TEXT NOT NULL, body TEXT NOT NULL CHECK(json_valid(body)), UNIQUE(kind,id));
                CREATE TABLE history(sequence INTEGER PRIMARY KEY AUTOINCREMENT,kind TEXT NOT NULL,id TEXT NOT NULL,body TEXT NOT NULL,revision INTEGER NOT NULL);
                CREATE TABLE events(sequence INTEGER PRIMARY KEY AUTOINCREMENT,revision INTEGER NOT NULL,action TEXT NOT NULL,at TEXT NOT NULL);
                PRAGMA user_version=5;")?;
            tx.execute_batch(derivative_files::CREATE_CATALOG)?;
            tx.commit()?;
        }
        conn.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;",
        )?;
        let integrity: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        require(integrity == "ok", "Workspace integrity check failed")?;
        let mut workspace = Self {
            root,
            conn,
            runtime: None,
        };
        if (1..SCHEMA).contains(&version) {
            // Older readers lack mapping or finding-review semantics. Retain a
            // complete recovery point before changing records or compatibility.
            workspace.backup()?;
            workspace.change(None, "workspace.schema_v5", version < 3, |conn| {
                conn.execute_batch(derivative_files::CREATE_CATALOG)?;
                conn.pragma_update(None, "user_version", SCHEMA)?;
                let actual: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
                require(actual == SCHEMA, "Schema upgrade postcondition failed")
            })?;
        }
        if workspace
            .conn
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?
            == SCHEMA
        {
            let _: u64 =
                workspace
                    .conn
                    .query_row("SELECT count(*) FROM derivative_objects", [], |r| r.get(0))?;
        }
        let interrupted: Vec<CollectionJob> = all::<CollectionJob>(&workspace.conn, "job")?
            .into_iter()
            .filter(|j| matches!(j.state, JobState::Running))
            .collect();
        if !interrupted.is_empty() {
            workspace.change(None,"jobs.interrupted",false,|conn|{
            for mut job in interrupted {job.state=JobState::Failed;job.detail="Interrupted before completion. Retained originals remain available; no automatic network retry was made.".into();put(conn,"job",&job.id,&job)?;}Ok(())
        })?;
        }
        Ok(workspace)
    }
    pub fn attach_runtime(&mut self, runtime: crate::engines::Runtime) {
        self.runtime = Some(runtime);
    }
    pub fn revision(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT revision FROM meta", [], |r| r.get(0))?)
    }
    fn change<F>(
        &mut self,
        expected: Option<u64>,
        action: &str,
        invalidate: bool,
        operation: F,
    ) -> Result<()>
    where
        F: FnOnce(&Connection) -> Result<()>,
    {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: u64 = tx.query_row("SELECT revision FROM meta", [], |r| r.get(0))?;
        if expected.is_some_and(|v| v != current) {
            return Err(Error::Conflict(
                "Workspace changed; reload before applying this decision".into(),
            ));
        }
        operation(&tx)?;
        if invalidate {
            for mut f in all::<Finding>(&tx, "finding")? {
                f.needs_review = true;
                put(&tx, "finding", &f.id, &f)?;
            }
        }
        tx.execute("UPDATE meta SET revision=revision+1", [])?;
        tx.execute(
            "INSERT INTO events(revision,action,at) VALUES(?,?,?)",
            params![current + 1, action, now()],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn view(&self) -> Result<WorkspaceView> {
        self.view_with_reports(|conn| all(conn, "report"))
    }
    fn view_with_reports<R>(
        &self,
        reports: impl FnOnce(&Connection) -> Result<Vec<R>>,
    ) -> Result<WorkspaceView<R>> {
        // Pin the revision and every table to one SQLite read snapshot.
        // A coordinator on another connection may commit while this view loads.
        let transaction = self.conn.unchecked_transaction()?;
        let view = WorkspaceView {
            schema_version: SCHEMA,
            revision: self.revision()?,
            entities: all(&transaction, "entity")?,
            evidence: all_evidence(&transaction)?,
            observations: all(&transaction, "observation")?,
            assertions: all(&transaction, "assertion")?,
            transactions: all(&transaction, "transaction")?,
            addresses: all(&transaction, "address")?,
            locations: all(&transaction, "location")?,
            leads: all(&transaction, "lead")?,
            jobs: all(&transaction, "job")?,
            findings: all(&transaction, "finding")?,
            hypotheses: all(&transaction, "hypothesis")?,
            decisions: all(&transaction, "decision")?,
            merges: all(&transaction, "merge")?,
            identity_decisions: all(&transaction, "identity_decision")?,
            reports: reports(&transaction)?,
            statement_profiles: all(&transaction, "statement_profile")?,
            statement_imports: all(&transaction, "statement_import")?,
        };
        transaction.commit()?;
        Ok(view)
    }
    pub fn dispatch(&mut self, command: Command) -> Result<Value> {
        self.dispatch_with_view(command, ResponseMode::Full)
    }
    /// Desktop response mode: immutable report bodies are fetched on explicit export.
    pub fn dispatch_presentation(&mut self, command: Command) -> Result<Value> {
        self.dispatch_with_view(command, ResponseMode::Presentation)
    }
    /// Opt-in smaller refresh response; desktop callers still use presentation mode.
    pub fn dispatch_summary(&mut self, command: Command) -> Result<Value> {
        self.dispatch_with_view(command, ResponseMode::Summary)
    }
    fn dispatch_with_view(&mut self, command: Command, mode: ResponseMode) -> Result<Value> {
        match command {
            Command::SaveDocxSnapshot {
                request_id,
                expected_revision,
            } => {
                return Ok(serde_json::to_value(
                    self.save_docx_snapshot(&request_id, expected_revision)?,
                )?);
            }
            Command::PageDocxSnapshots {
                request,
                expected_revision,
            } => {
                return Ok(serde_json::to_value(
                    self.page_docx_snapshots(&request, expected_revision)?,
                )?);
            }
            Command::InspectDocxSnapshot {
                report_id,
                expected_document_sha256,
                expected_docx_sha256,
            } => {
                return Ok(serde_json::to_value(self.inspect_docx_snapshot(
                    &report_id,
                    &expected_document_sha256,
                    &expected_docx_sha256,
                )?)?);
            }
            Command::ExportTransactions {
                request,
                expected_revision,
            } => {
                return Ok(serde_json::to_value(
                    self.export_transactions(&request, expected_revision)?,
                )?);
            }
            Command::PageTransferCandidates {
                request,
                expected_revision,
            } => {
                return Ok(serde_json::to_value(
                    self.page_transfer_candidates(&request, expected_revision)?,
                )?);
            }
            Command::ReadTransactionBalances {
                request,
                expected_revision,
            } => {
                return Ok(serde_json::to_value(
                    self.read_transaction_balances(&request, expected_revision)?,
                )?);
            }
            Command::PageCitationCatalogue {
                request,
                expected_revision,
            } => {
                return Ok(serde_json::to_value(
                    self.page_citation_catalogue(&request, expected_revision)?,
                )?);
            }
            Command::ReadCitationSelections {
                request,
                expected_revision,
            } => {
                return Ok(serde_json::to_value(
                    self.read_citation_selections(&request, expected_revision)?,
                )?);
            }
            Command::SearchTransactions {
                request,
                expected_revision,
            } => {
                return Ok(serde_json::to_value(
                    self.search_transactions(&request, expected_revision)?,
                )?);
            }
            Command::PageTransactionFacets {
                request,
                expected_revision,
            } => {
                return Ok(serde_json::to_value(
                    self.page_transaction_facets(&request, expected_revision)?,
                )?)
            }
            Command::PageReviewDecisions {
                request,
                expected_revision,
            } => {
                return Ok(serde_json::to_value(
                    self.page_review_decisions(&request, expected_revision)?,
                )?);
            }
            Command::ReadTransactionSources {
                request,
                expected_revision,
            } => {
                return Ok(serde_json::to_value(
                    self.read_transaction_sources(&request, expected_revision)?,
                )?)
            }
            Command::PageTransactions {
                request,
                expected_revision,
            } => {
                return Ok(serde_json::to_value(
                    self.page_transactions(&request, expected_revision)?,
                )?)
            }
            Command::InspectReportSnapshot {
                report_id,
                expected_sha256,
            } => {
                return Ok(serde_json::to_value(
                    self.inspect_report_snapshot(&report_id, &expected_sha256)?,
                )?);
            }
            Command::QueuePdfPageOcr {
                evidence_id,
                request_key,
                page_number,
                dpi,
            } => {
                return Ok(serde_json::to_value(self.queue_pdf_page_ocr(
                    &evidence_id,
                    &request_key,
                    page_number,
                    dpi,
                )?)?)
            }
            Command::InspectPdfExtraction { extraction_id } => {
                return Ok(serde_json::to_value(self.pdf_extraction(&extraction_id)?)?)
            }
            Command::QueueImageOcrRegions {
                evidence_id,
                request_key,
            } => {
                return Ok(serde_json::to_value(
                    self.queue_image_ocr_regions(&evidence_id, &request_key)?,
                )?);
            }
            Command::InspectImageRegionExtraction { extraction_id } => {
                return Ok(serde_json::to_value(
                    self.inspect_image_region_extraction(&extraction_id)?,
                )?);
            }
            Command::QueueImageOcr {
                evidence_id,
                request_key,
            } => {
                return Ok(serde_json::to_value(
                    self.queue_image_ocr(&evidence_id, &request_key)?,
                )?);
            }
            Command::InspectImageExtraction { extraction_id } => {
                return Ok(serde_json::to_value(
                    self.image_extraction(&extraction_id)?,
                )?);
            }
            Command::QueueDocumentParse {
                evidence_id,
                request_key,
            } => {
                return Ok(serde_json::to_value(
                    self.queue_document_parse(&evidence_id, &request_key)?,
                )?)
            }
            Command::ListProcessingJobs {} => {
                return Ok(serde_json::to_value(self.processing_jobs()?)?)
            }
            Command::InspectExtraction { extraction_id } => {
                return Ok(serde_json::to_value(self.extraction(&extraction_id)?)?)
            }
            Command::InspectProcessingJob { job_id } => {
                return Ok(serde_json::to_value(self.processing_job(&job_id)?)?)
            }
            Command::CancelProcessingJob {
                job_id,
                expected_attempt,
            } => {
                return Ok(serde_json::to_value(
                    self.cancel_processing_job(&job_id, expected_attempt)?,
                )?)
            }
            Command::RetryProcessingJob {
                job_id,
                expected_attempt,
                reason,
            } => {
                return Ok(serde_json::to_value(self.retry_processing_job(
                    &job_id,
                    expected_attempt,
                    &reason,
                )?)?)
            }
            Command::CompareTransactionPeriods {
                request,
                expected_revision,
            } => {
                return Ok(serde_json::to_value(
                    self.compare_transaction_periods(&request, expected_revision)?,
                )?);
            }
            Command::AnalyzeTransactions {
                request,
                expected_revision,
            } => {
                return Ok(serde_json::to_value(
                    self.analyze_transactions(&request, expected_revision)?,
                )?);
            }
            Command::View {} => {}
            Command::InspectSource { anchor } => {
                return Ok(serde_json::to_value(self.inspect_source(&anchor)?)?);
            }
            Command::Search { query } => {
                let runtime = self.runtime.as_ref().ok_or_else(|| {
                    Error::Blocked(
                        "Packaged local search runtime is not available in this build".into(),
                    )
                })?;
                let result = runtime.search(
                    &self.root.join("indexes/lucene"),
                    self.revision()?,
                    &all_evidence(&self.conn)?,
                    &query,
                )?;
                return Ok(serde_json::to_value(result)?);
            }
            Command::CollectWeb {
                urls,
                max_hops,
                max_requests,
                max_seconds,
            } => self.collect_web(urls, max_hops, max_requests, max_seconds)?,
            Command::InspectCollection { job_id } => {
                return Ok(serde_json::to_value(self.collection_receipt(&job_id)?)?);
            }
            Command::ExportCollection { job_id } => {
                return self.export_collection(&job_id);
            }
            Command::SeedDemo {} => self.seed_demo()?,
            Command::Import { name, bytes } => {
                self.import(&name, &bytes)?;
            }
            Command::InspectStatement { bytes, delimiter } => {
                return Ok(serde_json::to_value(crate::statements::sample(
                    &bytes, delimiter,
                )?)?);
            }
            Command::PreviewStatement {
                name,
                bytes,
                mapping,
            } => {
                return Ok(serde_json::to_value(
                    self.preview_statement(&name, &bytes, &mapping)?,
                )?);
            }
            Command::ImportStatement {
                name,
                bytes,
                mapping,
                preview_token,
                save_profile_name,
                expected_revision,
            } => {
                self.import_statement(
                    &name,
                    &bytes,
                    mapping,
                    &preview_token,
                    save_profile_name.as_deref(),
                    expected_revision,
                )?;
            }
            Command::AddEntity {
                entity,
                reason,
                expected_revision,
            } => {
                self.add_entity(entity, &reason, expected_revision)?;
            }
            Command::UpdateEntity {
                id,
                entity,
                reason,
                expected_revision,
            } => {
                self.update_entity(&id, entity, &reason, expected_revision)?;
            }
            Command::AddObservation {
                observation,
                reason,
                expected_revision,
            } => {
                self.add_observation(observation, &reason, expected_revision)?;
            }
            Command::CorrectObservation {
                id,
                value,
                anchor,
                reason,
                expected_revision,
            } => {
                self.correct_observation(&id, &value, anchor, &reason, expected_revision)?;
            }
            Command::ReviewObservation {
                id,
                state,
                reason,
                expected_revision,
            } => {
                self.review_observation(&id, state, &reason, expected_revision)?;
            }
            Command::CompareEntities { left_id, right_id } => {
                return Ok(serde_json::to_value(
                    self.compare_entities(&left_id, &right_id)?,
                )?);
            }
            Command::DecideIdentity {
                left_id,
                right_id,
                outcome,
                reason,
                expected_revision,
            } => {
                self.decide_identity(&left_id, &right_id, outcome, &reason, expected_revision)?;
            }
            Command::ReviewTransaction {
                id,
                state,
                reason,
                expected_revision,
            } => self.review_transaction(&id, state, &reason, expected_revision)?,
            Command::CorrectTransaction {
                id,
                amount,
                reason,
                expected_revision,
            } => self.correct_transaction(&id, &amount, &reason, expected_revision)?,
            Command::MatchTransfer {
                first,
                second,
                reason,
                expected_revision,
            } => self.match_transfer(&first, &second, &reason, expected_revision)?,
            Command::Merge {
                source,
                target,
                reason,
                expected_revision,
            } => self.merge(&source, &target, &reason, expected_revision)?,
            Command::ReverseMerge {
                id,
                reason,
                expected_revision,
            } => self.reverse_merge(&id, &reason, expected_revision)?,
            Command::AddQuestion {
                question,
                reason,
                expected_revision,
            } => {
                self.save_question(None, question, &reason, expected_revision)?;
            }
            Command::UpdateQuestion {
                id,
                question,
                reason,
                expected_revision,
            } => {
                self.save_question(Some(&id), question, &reason, expected_revision)?;
            }
            Command::AddFinding {
                title,
                assessment,
                supporting_ids,
                contradicting_ids,
                limitations,
                hypothesis_ids,
                expected_revision,
            } => {
                self.add_finding(
                    FindingInput {
                        title,
                        assessment,
                        supporting_ids,
                        contradicting_ids,
                        limitations,
                        hypothesis_ids,
                    },
                    expected_revision,
                )?;
            }
            Command::UpdateFinding {
                id,
                finding,
                reason,
                expected_revision,
            } => {
                self.update_finding(&id, finding, &reason, expected_revision)?;
            }
            Command::ReviewFinding {
                id,
                reason,
                expected_revision,
            } => {
                self.review_finding(&id, &reason, expected_revision)?;
            }
            Command::SaveReport {} => {
                self.save_report()?;
            }
            Command::Backup {} => {
                let path = self.backup()?;
                return Ok(json!({"backup":path.file_name().and_then(|s|s.to_str())}));
            }
        }
        match mode {
            ResponseMode::Full => workspace_response(self.view()?),
            ResponseMode::Presentation => workspace_response(self.presentation()?),
            ResponseMode::Summary => Ok(serde_json::to_value(self.desktop_summary()?)?),
        }
    }
    pub fn import(&mut self, name: &str, bytes: &[u8]) -> Result<String> {
        validate_import_input(name, bytes)?;
        let digest = hash(bytes);
        let existing = find_evidence(&self.conn, &digest)?;
        if existing
            .as_ref()
            .is_some_and(|e| e.extraction_status != "acquisition_only")
        {
            return Ok(digest);
        }
        let extension = Path::new(name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let is_text = ["txt", "csv"].contains(&extension.as_str());
        let text = if is_text {
            Some(
                std::str::from_utf8(bytes)
                    .map_err(|_| Error::Validation("Text must be valid UTF-8".into()))?
                    .to_string(),
            )
        } else {
            None
        };
        let mut transactions = vec![];
        if extension == "csv" {
            let sample = crate::statements::sample(bytes, crate::statements::Delimiter::Comma)?;
            let parsed = crate::statements::parse(bytes, &sample.suggested_mapping)?;
            require(
                parsed.invalid_rows == 0,
                "Transaction CSV contains invalid rows; use statement preview for row details",
            )?;
            transactions = parsed.transactions;
        }
        let evidence = Evidence {
            id: digest.clone(),
            name: name.into(),
            sha256: digest.clone(),
            bytes: bytes.len() as u64,
            media_type: if extension == "csv" {
                "text/csv"
            } else if is_text {
                "text/plain"
            } else {
                "application/octet-stream"
            }
            .into(),
            origin_group: digest.clone(),
            imported_at: now(),
            extraction_status: if is_text { "complete" } else { "unprocessed" }.into(),
            text,
            acquisitions: existing.map(|e| e.acquisitions).unwrap_or_default(),
        };
        retain_original(&self.root, &evidence, bytes)?;
        self.change(None, "evidence.import", true, |conn| {
            put(conn, "evidence", &digest, &evidence)?;
            for t in &transactions {
                put(conn, "transaction", &t.id, t)?;
            }
            refresh_duplicates(conn)
        })?;
        Ok(digest)
    }
    pub fn review_transaction(
        &mut self,
        key: &str,
        state: ReviewState,
        why: &str,
        expected: u64,
    ) -> Result<()> {
        reason(why)?;
        self.change(Some(expected), "transaction.review", true, |conn| {
            let mut t: Transaction = get(conn, "transaction", key)?;
            require(
                t.transfer_peer.is_none(),
                "Unpairing a transfer requires a dedicated correction workflow",
            )?;
            t.review = state.clone();
            t.version += 1;
            put(conn, "transaction", key, &t)?;
            record_decision(conn, key, state, why)
        })
    }
    pub fn correct_transaction(
        &mut self,
        key: &str,
        value: &str,
        why: &str,
        expected: u64,
    ) -> Result<()> {
        analytics::amount(value)?;
        reason(why)?;
        self.change(Some(expected), "transaction.correct", true, |conn| {
            let mut t: Transaction = get(conn, "transaction", key)?;
            if let Some(peer) = t.transfer_peer.take() {
                let mut p: Transaction = get(conn, "transaction", &peer)?;
                p.transfer_peer = None;
                p.version += 1;
                put(conn, "transaction", &peer, &p)?;
            }
            t.amount = value.into();
            t.review = ReviewState::Pending;
            t.version += 1;
            put(conn, "transaction", key, &t)?;
            refresh_duplicates(conn)?;
            record_decision(conn, key, ReviewState::Pending, why)
        })
    }
    pub fn match_transfer(
        &mut self,
        first: &str,
        second: &str,
        why: &str,
        expected: u64,
    ) -> Result<()> {
        reason(why)?;
        require(first != second, "A transaction cannot transfer to itself")?;
        self.change(Some(expected), "transfer.match", true, |conn| {
            let mut a: Transaction = get(conn, "transaction", first)?;
            let mut b: Transaction = get(conn, "transaction", second)?;
            require(
                a.account != b.account
                    && a.currency == b.currency
                    && a.transfer_peer.is_none()
                    && b.transfer_peer.is_none(),
                "Transfer accounts, currency or existing match conflict",
            )?;
            require(
                analytics::amount(&a.amount)? == -analytics::amount(&b.amount)?
                    && !analytics::amount(&a.amount)?.is_zero(),
                "Transfer amounts must be equal and opposite",
            )?;
            require(
                a.review == ReviewState::Accepted && b.review == ReviewState::Accepted,
                "Review both transactions before matching",
            )?;
            a.transfer_peer = Some(second.into());
            b.transfer_peer = Some(first.into());
            a.version += 1;
            b.version += 1;
            put(conn, "transaction", first, &a)?;
            put(conn, "transaction", second, &b)?;
            record_decision(conn, first, ReviewState::Accepted, why)
        })
    }
    pub fn merge(&mut self, source: &str, target: &str, why: &str, expected: u64) -> Result<()> {
        reason(why)?;
        require(source != target, "Cannot merge an entity with itself")?;
        self.change(Some(expected), "entity.merge", true, |conn| {
            let mut s: Entity = get(conn, "entity", source)?;
            let t: Entity = get(conn, "entity", target)?;
            require(
                s.kind == t.kind && s.merged_into.is_none() && t.merged_into.is_none(),
                "Merge requires two unmerged entities of the same kind",
            )?;
            let entities = all::<Entity>(conn, "entity")?;
            require(
                !entities
                    .iter()
                    .any(|e| e.merged_into.as_deref() == Some(source)),
                "Reverse dependent merges before merging their target",
            )?;
            s.merged_into = Some(target.into());
            put(conn, "entity", source, &s)?;
            let m = MergeDecision {
                id: id(),
                source: source.into(),
                target: target.into(),
                reason: why.into(),
                reversed: false,
            };
            put(conn, "merge", &m.id, &m)
        })
    }
    pub fn reverse_merge(&mut self, key: &str, why: &str, expected: u64) -> Result<()> {
        reason(why)?;
        self.change(Some(expected), "entity.merge.reverse", true, |conn| {
            let mut m: MergeDecision = get(conn, "merge", key)?;
            require(!m.reversed, "Merge already reversed")?;
            let mut e: Entity = get(conn, "entity", &m.source)?;
            require(
                e.merged_into.as_deref() == Some(&m.target),
                "Merge no longer matches current entity state",
            )?;
            e.merged_into = None;
            m.reversed = true;
            put(conn, "entity", &e.id, &e)?;
            put(conn, "merge", key, &m)?;
            record_decision(conn, key, ReviewState::Rejected, why)
        })
    }
    fn verify_original(&self, e: &Evidence) -> Result<()> {
        verify_original(&self.root, e)
    }
    pub fn seed_demo(&mut self) -> Result<()> {
        let has_records: bool =
            self.conn
                .query_row("SELECT EXISTS(SELECT 1 FROM records)", [], |row| row.get(0))?;
        if self.revision()? != 0 || has_records {
            return Err(Error::Conflict(
                "Load the demonstration only into a new, empty workspace".into(),
            ));
        }
        let evidence_id =
            self.import("brief.txt", include_bytes!("../../../fixtures/brief.txt"))?;
        let statement = self.import(
            "statement.csv",
            include_bytes!("../../../fixtures/statement.csv"),
        )?;
        self.change(None,"demo.seed",false,|conn|{
            for (key,name,kind,reference) in [("person-a","Rowan Ellis",EntityKind::Person,"000042"),("person-b","Rowan Ellis",EntityKind::Person,"000043"),("org-a","North Quay Cooperative",EntityKind::Organisation,"000101")] {
                let e=Entity{id:key.into(),name:name.into(),kind,identifiers:vec![Identifier{namespace:"DEMO".into(),value:reference.into()}],merged_into:None};put(conn,"entity",key,&e)?;
            }
            let anchor=|line|SourceAnchor::Text{evidence_id:evidence_id.clone(),line_start:line,line_end:line};
            for (key,entity,field,value,line) in [("obs-a","person-a","birth_year","1984",4),("obs-b","person-b","birth_year","1991",5),("obs-c","org-a","public_notice","Association mentioned; not yet corroborated",6)] {
                let o=Observation{id:key.into(),entity_id:entity.into(),field:field.into(),value:value.into(),anchor:anchor(line),extraction_quality:None,review:ReviewState::Pending};put(conn,"observation",key,&o)?;
            }
            let a=Assertion{id:"relationship-a".into(),subject_id:"person-a".into(),predicate:"mentioned alongside".into(),object_id:"org-a".into(),observation_ids:vec!["obs-c".into()],valid_from:Some("2025-03-01".into()),valid_to:None,confidence:"Unassessed".into(),review:ReviewState::Pending};put(conn,"assertion",&a.id,&a)?;
            let h=Hypothesis{id:"question-a".into(),question:"What relationship is supported by the records?".into(),proposition:"The two subjects may be associated".into(),alternatives:vec!["A namesake accounts for the mention".into(),"The copied notice repeats an unsupported statement".into()],gaps:vec!["Independent confirmation of identity".into(),"Historical merchant branch evidence".into()]};put(conn,"hypothesis",&h.id,&h)?;
            let lead=Lead{id:"lead-a".into(),label:"Confirm the original public notice".into(),identifier:Identifier{namespace:"organisation".into(),value:"North Quay Cooperative".into()},source_id:Some(evidence_id.clone()),state:ReviewState::Pending};put(conn,"lead",&lead.id,&lead)?;
            for (key,label,lat,lon,start,end) in [("address-a","Fictional previous reference",-34.92,138.60,"2024-01-01",Some("2025-03-04")),("address-b","Fictional current reference",-34.90,138.62,"2025-03-05",None)] {
                let a=AddressAssociation{id:key.into(),entity_id:"person-a".into(),label:label.into(),latitude:lat,longitude:lon,valid_from:start.into(),valid_to:end.map(str::to_string),uncertainty_m:100.0,anchor:anchor(8)};put(conn,"address",key,&a)?;
            }
            for (key,branch,lat,lon) in [("branch-a","Branch A",-34.921,138.607),("branch-b","Branch B",-34.887,138.63)] {
                let m=MerchantLocation{id:key.into(),transaction_id:format!("{statement}:3"),merchant:"North Quay Market".into(),branch:Some(branch.into()),channel:Channel::InPerson,latitude:Some(lat),longitude:Some(lon),uncertainty_m:50.0,retrieved_at:"2025-03-10".into(),valid_from:None,valid_to:None,anchor:anchor(9),review:ReviewState::Pending};put(conn,"location",key,&m)?;
            }
            let f=Finding{id:"finding-a".into(),hypothesis_ids:vec!["question-a".into()],title:"A café amount requires source review".into(),assessment:"The imported debit does not reconcile with the adjacent balances. The original remains unchanged.".into(),supporting_ids:vec![format!("{statement}:10")],contradicting_ids:vec![],limitations:"Synthetic demonstration; review the source before accepting any correction.".into(),needs_review:true};put(conn,"finding",&f.id,&f)?;
            Ok(())
        })
    }
}

// Bounded candidate examples, linear grouping. Repeated transactions are retained.
fn refresh_duplicates(conn: &Connection) -> Result<()> {
    use std::collections::HashMap;
    let transactions = all::<Transaction>(conn, "transaction")?;
    let key = |t: &Transaction| -> Result<String> {
        Ok(serde_json::to_string(&(
            &t.account,
            &t.date,
            &t.description,
            &t.currency,
            analytics::amount(&t.amount)?.normalize().to_string(),
        ))?)
    };
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for t in &transactions {
        let ids = groups.entry(key(t)?).or_default();
        if ids.len() < 51 {
            ids.push(t.id.clone());
        }
    }
    for mut t in transactions {
        let candidates = groups
            .get(&key(&t)?)
            .map(|ids| {
                ids.iter()
                    .filter(|id| *id != &t.id)
                    .take(50)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if candidates != t.duplicate_candidates {
            t.duplicate_candidates = candidates;
            put(conn, "transaction", &t.id, &t)?;
        }
    }
    Ok(())
}

fn validate_import_input(name: &str, bytes: &[u8]) -> Result<()> {
    require(
        !bytes.is_empty() && bytes.len() <= policy::MAX_IMPORT_BYTES,
        "Import must contain 1 byte to 16 MiB",
    )?;
    require(
        !name.is_empty()
            && name.len() <= 180
            && !name.contains(['/', '\\', ':'])
            && !name.chars().any(char::is_control),
        "Invalid display filename",
    )?;
    Ok(())
}

fn verify_original(root: &Path, e: &Evidence) -> Result<()> {
    read_original(root, e).map(|_| ())
}

fn retain_original(root: &Path, evidence: &Evidence, bytes: &[u8]) -> Result<()> {
    let path = root.join("originals").join(&evidence.sha256);
    reject_link_ancestors(&path)?;
    if path.exists() {
        return verify_original(root, evidence);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    private_file(&path, 0o600)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    private_file(&path, 0o400)
}

fn workspace_response<R: Serialize>(view: WorkspaceView<R>) -> Result<Value> {
    Ok(json!({"analysis":analytics::analyse(&view.transactions)?,"workspace":view}))
}
