//! Canonical publication of analyst-previewed statement mappings and imports.
use super::*;
use crate::statements::*;

fn preview_token(
    name: &str,
    digest: &str,
    mapping: &StatementMapping,
    revision: u64,
) -> Result<String> {
    Ok(hash(&serde_json::to_vec(
        &json!({"name":name,"sha256":digest,"mapping":mapping,"revision":revision}),
    )?))
}
fn evidence_exists(conn: &Connection, digest: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM records WHERE kind='evidence' AND id=?)",
        [digest],
        |r| r.get(0),
    )?)
}
pub(super) fn source_delimiter(conn: &Connection, evidence_id: &str) -> Result<Delimiter> {
    let body: Option<String> = conn
        .query_row(
            "SELECT body FROM records WHERE kind='statement_import' AND id=?",
            [evidence_id],
            |r| r.get(0),
        )
        .optional()?;
    match body {
        Some(body) => Ok(serde_json::from_str::<StatementImport>(&body)?
            .mapping
            .delimiter),
        None => Ok(Delimiter::Comma),
    }
}
impl Workspace {
    pub fn preview_statement(
        &self,
        name: &str,
        bytes: &[u8],
        mapping: &StatementMapping,
    ) -> Result<StatementPreview> {
        validate_import_input(name, bytes)?;
        let parsed = crate::statements::parse(bytes, mapping)?;
        let revision = self.revision()?;
        let digest = hash(bytes);
        let balance_mismatches = analytics::analyse(&parsed.transactions)?
            .balance_checks
            .iter()
            .filter(|c| !c.reconciled)
            .count();
        Ok(StatementPreview {
            workspace_revision: revision,
            preview_token: preview_token(name, &digest, mapping, revision)?,
            already_imported: evidence_exists(&self.conn, &digest)?,
            sha256: digest,
            total_rows: parsed.total_rows,
            valid_rows: parsed.transactions.len(),
            invalid_rows: parsed.invalid_rows,
            balance_mismatches,
            rows_truncated: parsed.total_rows > parsed.rows.len(),
            issues_truncated: parsed.invalid_rows > parsed.issues.len(),
            rows: parsed.rows,
            issues: parsed.issues,
        })
    }
    pub fn import_statement(
        &mut self,
        name: &str,
        bytes: &[u8],
        mapping: StatementMapping,
        token: &str,
        profile_name: Option<&str>,
        expected: u64,
    ) -> Result<String> {
        validate_import_input(name, bytes)?;
        let digest = hash(bytes);
        require(
            token == preview_token(name, &digest, &mapping, expected)?,
            "Statement or mapping changed; generate a new preview before importing",
        )?;
        let parsed = crate::statements::parse(bytes, &mapping)?;
        require(
            parsed.invalid_rows == 0,
            "Resolve all source-row errors before importing; no rows were imported",
        )?;
        // Check reconciliation arithmetic before publishing any evidence or rows.
        analytics::analyse(&parsed.transactions)?;
        if let Some(name) = profile_name {
            require(
                !name.trim().is_empty()
                    && name.trim() == name
                    && name.len() <= 100
                    && !name.chars().any(char::is_control),
                "Profile name must be trimmed text of 1 to 100 bytes",
            )?;
        }
        let evidence = Evidence {
            id: digest.clone(),
            sha256: digest.clone(),
            name: name.into(),
            bytes: bytes.len() as u64,
            media_type: "text/csv".into(),
            origin_group: digest.clone(),
            imported_at: now(),
            extraction_status: "complete".into(),
            text: Some(
                std::str::from_utf8(bytes)
                    .map_err(|_| Error::Validation("Statement must be UTF-8".into()))?
                    .into(),
            ),
            acquisitions: vec![],
        };
        let root = self.root.clone();
        self.change(Some(expected), "statement.import", true, |conn| {
            require(
                !evidence_exists(conn, &digest)?,
                "This original is already retained; importing it again cannot add duplicate transactions",
            )?;
            let profiles = all::<StatementProfile>(conn, "statement_profile")?;
            let profile_id = if let Some(name) = profile_name {
                require(
                    !profiles.iter().any(|p| p.name.eq_ignore_ascii_case(name)),
                    "A profile with this name already exists; choose a new name or reuse it without saving another",
                )?;
                let profile = StatementProfile {
                    id: id(),
                    name: name.into(),
                    mapping: mapping.clone(),
                    created_at: now(),
                };
                put(conn, "statement_profile", &profile.id, &profile)?;
                Some(profile.id)
            } else {
                profiles.iter().find(|p| p.mapping == mapping).map(|p| p.id.clone())
            };
            retain_original(&root, &evidence, bytes)?;
            put(conn, "evidence", &digest, &evidence)?;
            for transaction in &parsed.transactions {
                put(conn, "transaction", &transaction.id, transaction)?;
            }
            let import = StatementImport {
                evidence_id: digest.clone(),
                mapping,
                profile_id,
                transaction_ids: parsed.transactions.iter().map(|t| t.id.clone()).collect(),
                imported_at: now(),
            };
            put(conn, "statement_import", &digest, &import)?;
            refresh_duplicates(conn)
        })?;
        Ok(digest)
    }
}
