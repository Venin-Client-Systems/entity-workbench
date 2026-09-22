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

const SCHEMA: u32 = 1;
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
fn private_dir(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        if ancestor.exists() {
            require(
                !fs::symlink_metadata(ancestor)?.file_type().is_symlink(),
                "Workspace path contains a symbolic link",
            )?;
        }
    }
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
                PRAGMA user_version=1;")?;
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
        Ok(WorkspaceView {
            schema_version: SCHEMA,
            revision: self.revision()?,
            entities: all(&self.conn, "entity")?,
            evidence: all(&self.conn, "evidence")?,
            observations: all(&self.conn, "observation")?,
            assertions: all(&self.conn, "assertion")?,
            transactions: all(&self.conn, "transaction")?,
            addresses: all(&self.conn, "address")?,
            locations: all(&self.conn, "location")?,
            leads: all(&self.conn, "lead")?,
            jobs: all(&self.conn, "job")?,
            findings: all(&self.conn, "finding")?,
            hypotheses: all(&self.conn, "hypothesis")?,
            decisions: all(&self.conn, "decision")?,
            merges: all(&self.conn, "merge")?,
            reports: all(&self.conn, "report")?,
        })
    }
    pub fn dispatch(&mut self, command: Command) -> Result<Value> {
        match command {
            Command::View {} => {}
            Command::Search { query } => {
                let runtime = self.runtime.as_ref().ok_or_else(|| {
                    Error::Blocked(
                        "Packaged local search runtime is not available in this build".into(),
                    )
                })?;
                let result = runtime.search(
                    &self.root.join("indexes/lucene"),
                    self.revision()?,
                    &all::<Evidence>(&self.conn, "evidence")?,
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
            Command::SeedDemo {} => self.seed_demo()?,
            Command::Import { name, bytes } => {
                self.import(&name, &bytes)?;
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
            Command::AddFinding {
                title,
                assessment,
                supporting_ids,
                contradicting_ids,
                limitations,
                expected_revision,
            } => {
                reason(&title)?;
                reason(&assessment)?;
                reason(&limitations)?;
                self.change(Some(expected_revision),"finding.add",false,|conn|{
                    require(!supporting_ids.is_empty()||!contradicting_ids.is_empty(),"Cite at least one evidence item, observation or transaction")?;
                    for key in supporting_ids.iter().chain(&contradicting_ids) {
                        let count:u32=conn.query_row("SELECT count(*) FROM records WHERE id=? AND kind IN ('evidence','observation','transaction')",[key],|r|r.get(0))?;
                        require(count==1,"Finding citation is unresolved")?;
                    }
                    let finding=Finding{id:id(),title,assessment,supporting_ids,contradicting_ids,limitations,needs_review:false};
                    put(conn,"finding",&finding.id,&finding)
                })?;
            }
            Command::SaveReport {} => {
                self.save_report()?;
            }
            Command::Backup {} => {
                let path = self.backup()?;
                return Ok(json!({"backup":path.file_name().and_then(|s|s.to_str())}));
            }
        }
        let view = self.view()?;
        Ok(json!({"analysis":analytics::analyse(&view.transactions)?,"workspace":view}))
    }
    pub fn import(&mut self, name: &str, bytes: &[u8]) -> Result<String> {
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
        let digest = hash(bytes);
        if get::<Evidence>(&self.conn, "evidence", &digest).is_ok() {
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
            let mut reader = csv::ReaderBuilder::new().from_reader(bytes);
            let headers = reader.headers()?.clone();
            let column = |field: &str| {
                headers.iter().position(|h| h == field).ok_or_else(|| {
                    Error::Validation(format!("CSV mapping requires column '{field}'"))
                })
            };
            let (account, date, description, amount, currency) = (
                column("account")?,
                column("date")?,
                column("description")?,
                column("amount")?,
                column("currency")?,
            );
            let optional = |record: &csv::StringRecord, field: &str| {
                headers
                    .iter()
                    .position(|h| h == field)
                    .and_then(|i| record.get(i))
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            };
            for (index, row) in reader.records().enumerate() {
                require(index < 100_000, "CSV row limit exceeded")?;
                let row = row?;
                let t = Transaction {
                    id: format!("{digest}:{}", index + 2),
                    account: row[account].into(),
                    date: row[date].into(),
                    posting_date: optional(&row, "posting_date"),
                    description: row[description].into(),
                    amount: row[amount].into(),
                    currency: row[currency].into(),
                    balance: optional(&row, "balance"),
                    anchor: SourceAnchor::Cell {
                        evidence_id: digest.clone(),
                        sheet: "CSV".into(),
                        row: (index + 2) as u32,
                        column: "amount".into(),
                    },
                    review: ReviewState::Pending,
                    duplicate_candidates: vec![],
                    transfer_peer: None,
                    merchant: None,
                    version: 1,
                };
                analytics::validate_transaction(&t)?;
                transactions.push(t);
            }
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
            extraction_status: if is_text {
                "complete"
            } else {
                "unsupported_in_development_build"
            }
            .into(),
            text,
            acquisitions: vec![],
        };
        let path = self.root.join("originals").join(&digest);
        if path.exists() {
            self.verify_original(&evidence)?;
        } else {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            private_file(&path, 0o600)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            private_file(&path, 0o400)?;
        }
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
        require(
            e.sha256.len() == 64 && e.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid evidence digest",
        )?;
        let path = self.root.join("originals").join(&e.sha256);
        let meta = fs::symlink_metadata(&path)?;
        require(
            meta.is_file() && !meta.file_type().is_symlink() && meta.len() == e.bytes,
            "Missing or altered original evidence",
        )?;
        require(
            hash(&fs::read(path)?) == e.sha256,
            "Original evidence checksum mismatch",
        )
    }
    pub fn backup(&mut self) -> Result<PathBuf> {
        let evidence = all::<Evidence>(&self.conn, "evidence")?;
        for e in &evidence {
            self.verify_original(e)?;
        }
        let path = self.root.join("backups").join(id());
        private_dir(&path)?;
        private_dir(&path.join("originals"))?;
        self.conn.backup("main", path.join("workspace.db"), None)?;
        private_file(&path.join("workspace.db"), 0o600)?;
        for e in &evidence {
            let target = path.join("originals").join(&e.sha256);
            fs::copy(self.root.join("originals").join(&e.sha256), &target)?;
            private_file(&target, 0o400)?;
        }
        fs::write(
            path.join("manifest.json"),
            serde_json::to_vec_pretty(
                &json!({"schema_version":SCHEMA,"revision":self.revision()?,"evidence":evidence.iter().map(|e|&e.sha256).collect::<Vec<_>>()}),
            )?,
        )?;
        private_file(&path.join("manifest.json"), 0o600)?;
        Ok(path)
    }
    pub fn restore(backup: &Path, destination: &Path) -> Result<Self> {
        require(!destination.exists(), "Restore destination must not exist")?;
        let source = Self {
            root: backup.to_path_buf(),
            runtime: None,
            conn: Connection::open_with_flags(
                backup.join("workspace.db"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?,
        };
        let version: u32 = source
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        require(version == SCHEMA, "Backup schema is unsupported")?;
        let evidence = all::<Evidence>(&source.conn, "evidence")?;
        for e in &evidence {
            source.verify_original(e)?;
        }
        private_dir(destination)?;
        private_dir(&destination.join("originals"))?;
        source
            .conn
            .backup("main", destination.join("workspace.db"), None)?;
        for e in &evidence {
            let target = destination.join("originals").join(&e.sha256);
            fs::copy(backup.join("originals").join(&e.sha256), &target)?;
            private_file(&target, 0o400)?;
        }
        Self::open(destination)
    }
    pub fn save_report(&mut self) -> Result<String> {
        let view = self.view()?;
        for e in &view.evidence {
            self.verify_original(e)?;
        }
        let report_id = id();
        let html = report::html(&view, &report_id)?;
        let snapshot = ReportSnapshot {
            id: report_id.clone(),
            workspace_revision: view.revision,
            created_at: now(),
            sha256: hash(html.as_bytes()),
            html,
        };
        let path = self.root.join("exports").join(format!("{report_id}.html"));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        private_file(&path, 0o600)?;
        file.write_all(snapshot.html.as_bytes())?;
        file.sync_all()?;
        self.change(None, "report.snapshot", false, |conn| {
            put(conn, "report", &report_id, &snapshot)
        })?;
        Ok(report_id)
    }
    pub fn seed_demo(&mut self) -> Result<()> {
        if !all::<Entity>(&self.conn, "entity")?.is_empty() {
            return Err(Error::Conflict(
                "Load the demonstration only into an empty entity workspace".into(),
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
            let f=Finding{id:"finding-a".into(),title:"A café amount requires source review".into(),assessment:"The imported debit does not reconcile with the adjacent balances. The original remains unchanged.".into(),supporting_ids:vec![format!("{statement}:10")],contradicting_ids:vec![],limitations:"Synthetic demonstration; review the source before accepting any correction.".into(),needs_review:true};put(conn,"finding",&f.id,&f)?;
            Ok(())
        })
    }
}

impl Workspace {
    pub fn collect_web(
        &mut self,
        urls: Vec<String>,
        hops: u32,
        requests: u32,
        seconds: u64,
    ) -> Result<()> {
        crate::collection::validate_seeds(&urls)?;
        require(
            hops <= 2 && requests > 0 && requests <= 50 && seconds > 0 && seconds <= 600,
            "Collection limits exceed policy",
        )?;
        let mut job = CollectionJob {
            id: id(),
            queries: urls.clone(),
            adapters: vec!["direct_web".into()],
            max_hops: hops,
            max_requests: requests,
            max_seconds: seconds,
            requests_used: 0,
            state: JobState::Running,
            detail: "Analyst selected direct website collection".into(),
        };
        self.change(None, "collection.start", false, |conn| {
            put(conn, "job", &job.id, &job)
        })?;
        match crate::collection::collect(urls, hops, requests, seconds) {
            Ok(result) => {
                job.requests_used = result.requests;
                job.state = result.state;
                job.detail = format!(
                    "{} pages retained. {}",
                    result.pages.len(),
                    result.notes.join("; ")
                );
                for page in result.pages {
                    if let Err(error) = self.retain_page(&job.id, page) {
                        job.state = JobState::Failed;
                        job.detail = format!(
                            "Collection storage failed; earlier evidence retained. {error}"
                        );
                        break;
                    }
                }
            }
            Err(error) => {
                job.state = JobState::Failed;
                job.detail = error.to_string();
            }
        }
        self.change(None, "collection.finish", false, |conn| {
            put(conn, "job", &job.id, &job)
        })
    }
    fn retain_page(&mut self, job_id: &str, page: crate::collection::Page) -> Result<()> {
        let digest = self.import("captured-page.html", &page.bytes)?;
        self.change(None, "collection.derivative", true, |conn| {
            let mut evidence: Evidence = get(conn, "evidence", &digest)?;
            evidence.text = Some(page.text);
            evidence.media_type = "text/html".into();
            // Identical bytes do not become independent sources just because
            // they were retrieved at another URL. Keep every acquisition.
            evidence.acquisitions.push(Acquisition {
                job_id: job_id.into(),
                url: page.url,
                retrieved_at: now(),
            });
            evidence.extraction_status = "static_text_only".into();
            put(conn, "evidence", &digest, &evidence)
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

#[cfg(test)]
mod acquisition_tests {
    use super::*;
    #[test]
    fn identical_captures_keep_both_urls_and_their_original_group() {
        let temp = tempfile::TempDir::new_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut w = Workspace::open(temp.path().join("case")).unwrap();
        for host in ["one.example", "two.example"] {
            w.retain_page(
                "test-job",
                crate::collection::Page {
                    url: format!("https://{host}/"),
                    bytes: b"<p>Copied synthetic source</p>".to_vec(),
                    text: "Copied synthetic source".into(),
                },
            )
            .unwrap();
        }
        let evidence = w.view().unwrap().evidence;
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].origin_group, evidence[0].sha256);
        assert_eq!(evidence[0].acquisitions.len(), 2);
        assert_eq!(evidence[0].acquisitions[0].url, "https://one.example/");
        assert_eq!(evidence[0].acquisitions[1].url, "https://two.example/");
        assert!(!evidence[0].acquisitions[0].retrieved_at.is_empty());
    }
}
