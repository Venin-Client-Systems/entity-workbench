//! Revision-bound metadata catalogue; artifact integrity is verified by explicit inspection/save.
use super::*;
use serde::Deserialize;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    schema_version: u32,
    query_sha256: String,
    sequence: i64,
}
impl Cursor {
    fn encode(&self) -> Result<String> {
        Ok(serde_json::to_vec(self)?
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
    fn decode(value: &str, query: &str) -> Result<Self> {
        require(
            value.len() <= MAX_DOCX_CURSOR_BYTES
                && value.len().is_multiple_of(2)
                && value
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            "Invalid DOCX catalogue cursor encoding",
        )?;
        let bytes = (0..value.len())
            .step_by(2)
            .map(|index| {
                u8::from_str_radix(&value[index..index + 2], 16)
                    .map_err(|_| Error::Validation("Invalid DOCX catalogue cursor".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let cursor: Self = serde_json::from_slice(&bytes)?;
        require(
            cursor.schema_version == 1 && cursor.query_sha256 == query && cursor.sequence > 0,
            "DOCX catalogue cursor belongs to another page size or revision",
        )?;
        Ok(cursor)
    }
}
impl Workspace {
    pub fn page_docx_snapshots(
        &self,
        request: &DocxSnapshotPageRequest,
        expected_revision: u64,
    ) -> Result<DocxSnapshotPage> {
        request.validate()?;
        let snapshot = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        if revision != expected_revision {
            return Err(Error::Conflict(
                "DOCX catalogue revision changed; refresh and restart the catalogue".into(),
            ));
        }
        let query_sha256 = hash(&serde_json::to_vec(&(
            "docx_snapshot_catalogue_v1",
            revision,
            request.page_size,
            "sequence_descending",
        ))?);
        let cursor = request
            .cursor
            .as_ref()
            .map(|c| Cursor::decode(c, &query_sha256))
            .transpose()?;
        let total_count: u64 =
            snapshot.query_row("SELECT count(*) FROM records WHERE kind=?", [KIND], |row| {
                row.get(0)
            })?;
        if let Some(cursor) = &cursor {
            let exists: bool = snapshot.query_row(
                "SELECT EXISTS(SELECT 1 FROM records WHERE kind=? AND sequence=?)",
                params![KIND, cursor.sequence],
                |r| r.get(0),
            )?;
            require(
                exists,
                "DOCX catalogue cursor no longer identifies a publication",
            )?;
        }
        let mut statement = snapshot.prepare("SELECT sequence,length(CAST(id AS BLOB)),length(CAST(body AS BLOB)) FROM records WHERE kind=?1 AND (?2 IS NULL OR sequence<?2) ORDER BY sequence DESC LIMIT ?3")?;
        let mut selected = statement.query(params![
            KIND,
            cursor.as_ref().map(|c| c.sequence),
            request.page_size + 1
        ])?;
        let mut positions = Vec::new();
        let mut retained_bytes = 0u64;
        let mut more = false;
        while let Some(row) = selected.next()? {
            if positions.len() == request.page_size as usize {
                more = true;
                break;
            }
            let sequence: i64 = row.get(0)?;
            let id_bytes: u64 = row.get(1)?;
            let body_bytes: u64 = row.get(2)?;
            require(
                sequence > 0 && id_bytes == 36 && body_bytes <= MAX_RECORD_BYTES,
                "DOCX catalogue row has an invalid identity or exceeds its metadata bound",
            )?;
            retained_bytes = retained_bytes
                .checked_add(body_bytes)
                .ok_or_else(|| Error::Validation("DOCX catalogue size overflow".into()))?;
            require(
                retained_bytes <= MAX_DOCX_CATALOGUE_BYTES,
                "DOCX catalogue exceeds its retained-body page bound; no partial page returned",
            )?;
            positions.push(sequence);
        }
        drop(selected);
        drop(statement);
        let mut rows = Vec::with_capacity(positions.len());
        for sequence in &positions {
            let id: String = snapshot.query_row(
                "SELECT id FROM records WHERE kind=? AND sequence=?",
                params![KIND, sequence],
                |r| r.get(0),
            )?;
            let record = lookup(&snapshot, &id)?
                .ok_or_else(|| Error::Validation("Missing DOCX catalogue row".into()))?;
            require(
                record.workspace_revision < revision,
                "DOCX catalogue source revision is not historical",
            )?;
            for reference in refs(&record) {
                files::verify_object_catalog(&snapshot, &reference)?;
            }
            rows.push(record);
        }
        let next_cursor = if more {
            Some(
                Cursor {
                    schema_version: 1,
                    query_sha256: query_sha256.clone(),
                    sequence: *positions.last().ok_or_else(|| {
                        Error::Validation("DOCX catalogue continuation has no preceding row".into())
                    })?,
                }
                .encode()?,
            )
        } else {
            None
        };
        snapshot.commit()?;
        Ok(DocxSnapshotPage {
            schema_version: 1,
            workspace_revision: revision,
            total_count,
            query_sha256,
            rows,
            next_cursor,
        })
    }
}

#[cfg(test)]
mod tests;
