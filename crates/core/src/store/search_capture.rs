//! One bounded read snapshot; no cache, original-file or execution side effects.
use super::*;
use crate::engines::search_corpus::{CorpusBuilder, SearchCorpus, MAX_EVIDENCE_ROWS};
use rusqlite::types::ValueRef;

pub(super) const MAX_ROW_BYTES: usize = 17 * 1024 * 1024;
pub(super) const MAX_AGGREGATE_BYTES: usize = 64 * 1024 * 1024;

fn text(value: ValueRef<'_>) -> Result<&[u8]> {
    match value {
        ValueRef::Text(bytes) => Ok(bytes),
        _ => Err(Error::Validation(
            "Search evidence metadata must be SQLite text".into(),
        )),
    }
}
impl Workspace {
    pub(super) fn capture_search(&self) -> Result<SearchCorpus> {
        let transaction = self.conn.unchecked_transaction()?;
        // This first read pins the snapshot before any evidence query is prepared.
        let revision = transaction.query_row("SELECT revision FROM meta", [], |row| row.get(0))?;
        let mut builder = CorpusBuilder::new(revision)?;
        {
            let mut statement = transaction.prepare(
                "SELECT id,body FROM records WHERE kind='evidence' ORDER BY sequence LIMIT ?1",
            )?;
            let mut rows = statement.query([MAX_EVIDENCE_ROWS + 1])?;
            let mut count = 0usize;
            let mut aggregate = 0usize;
            while let Some(row) = rows.next()? {
                require(
                    count < MAX_EVIDENCE_ROWS,
                    "Search evidence row limit exceeded",
                )?;
                count += 1;
                // Borrow first: no row.get::<String>, owned JSON Value, or decode before admission.
                let key = text(row.get_ref(0)?)?;
                let body = text(row.get_ref(1)?)?;
                require(key.len() == 64, "Search evidence key is not a digest")?;
                require(
                    body.len() <= MAX_ROW_BYTES,
                    "Search evidence metadata row limit exceeded",
                )?;
                aggregate = aggregate
                    .checked_add(body.len())
                    .filter(|size| *size <= MAX_AGGREGATE_BYTES)
                    .ok_or_else(|| {
                        Error::Validation(
                            "Search evidence aggregate metadata limit exceeded".into(),
                        )
                    })?;
                let key = std::str::from_utf8(key)
                    .map_err(|_| Error::Validation("Search evidence key is not UTF-8".into()))?;
                let body = std::str::from_utf8(body).map_err(|_| {
                    Error::Validation("Search evidence metadata is not UTF-8".into())
                })?;
                // Reuse the complete typed decoder, including all metadata and acquisition shapes.
                // Only one admitted Evidence allocation survives at a time; no corpus-wide Vec<Evidence>.
                let evidence = evidence::decode(key, body)?;
                builder.push(&evidence)?;
            }
        }
        transaction.commit()?;
        builder.finish()
    }
}

#[cfg(test)]
#[path = "search_capture_tests.rs"]
mod tests;
