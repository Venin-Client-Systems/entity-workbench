//! Shared metadata projection; no evidence text, originals, or canonical writes.
use super::*;
use crate::{
    citation_catalogue::*,
    literal_search::{lower_query, matches_text, LiteralMatching},
};
use serde::Deserialize;
use std::collections::BTreeSet;

// This is a fixed query, not an analyst expression. SQLite constructs the small
// projection; Rust checks its byte length before borrowing/deserializing it.
// SQLite still reads/parses retained JSON, including large omitted source text.
fn projection_sql() -> String {
    let source = "json_object('id',json_extract(s.body,'$.id'),
        'name',json_extract(s.body,'$.name'),'sha256',json_extract(s.body,'$.sha256'),
        'bytes',json_extract(s.body,'$.bytes'),'media_type',json_extract(s.body,'$.media_type'),
        'origin_group',json_extract(s.body,'$.origin_group'),
        'imported_at',json_extract(s.body,'$.imported_at'),
        'extraction_status',json_extract(s.body,'$.extraction_status'))";
    format!("WITH projected AS (
      SELECT c.sequence,c.id,c.kind,
       CASE c.kind WHEN 'observation' THEN 0 WHEN 'transaction' THEN 1 ELSE 2 END AS kind_rank,
       (SELECT count(*) FROM records other WHERE other.id=c.id AND other.kind IN
        ('observation','transaction','evidence')) AS identities,
       coalesce(s.id=json_extract(s.body,'$.id') AND json_type(s.body,'$.id')='text',0) AS source_valid,
       (e.sequence IS NULL OR coalesce(e.id=json_extract(e.body,'$.id')
         AND json_type(e.body,'$.id')='text' AND json_type(e.body,'$.name')='text',0)) AS entity_valid,
       CASE c.kind
       WHEN 'observation' THEN json_object('kind',c.kind,'id',json_extract(c.body,'$.id'),
        'entity_id',json_extract(c.body,'$.entity_id'),'entity_name',json_extract(e.body,'$.name'),
        'field',json_extract(c.body,'$.field'),'value',json_extract(c.body,'$.value'),
        'review',json_extract(c.body,'$.review'),'anchor',json_extract(c.body,'$.anchor'),'source',{source})
       WHEN 'transaction' THEN json_object('kind',c.kind,'id',json_extract(c.body,'$.id'),
        'description',json_extract(c.body,'$.description'),'amount',json_extract(c.body,'$.amount'),
        'currency',json_extract(c.body,'$.currency'),'date',json_extract(c.body,'$.date'),
        'account',json_extract(c.body,'$.account'),'review',json_extract(c.body,'$.review'),
        'version',json_extract(c.body,'$.version'),'anchor',json_extract(c.body,'$.anchor'),'source',{source})
       ELSE json_object('kind',c.kind,'id',json_extract(c.body,'$.id'),'source',{source}) END AS projection
      FROM records c
      LEFT JOIN records s ON s.kind='evidence' AND s.id=CASE c.kind WHEN 'evidence' THEN c.id
        ELSE json_extract(c.body,'$.anchor.evidence_id') END
      LEFT JOIN records e ON c.kind='observation' AND e.kind='entity'
        AND e.id=json_extract(c.body,'$.entity_id')
      WHERE c.kind IN ('observation','transaction','evidence'))")
}
const META: &str =
    "kind_rank,sequence,id,identities,source_valid,entity_valid,length(CAST(projection AS BLOB))";
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct Position {
    kind_rank: u8,
    sequence: i64,
}
struct Metadata {
    position: Position,
    id: String,
    bytes: usize,
}
fn text<'a>(row: &'a rusqlite::Row<'_>, index: usize) -> Result<&'a str> {
    match row.get_ref(index)? {
        rusqlite::types::ValueRef::Text(bytes) => std::str::from_utf8(bytes)
            .map_err(|_| Error::Validation("Citation projection is not UTF-8 text".into())),
        _ => Err(Error::Validation(
            "Citation projection has an invalid SQL type".into(),
        )),
    }
}
fn metadata(row: &rusqlite::Row<'_>) -> Result<Metadata> {
    let key = text(row, 2)?;
    identifier(key)?;
    require(
        row.get::<_, u64>(3)? == 1,
        "Citation identity is ambiguous across supported kinds",
    )?;
    require(
        row.get(4)?,
        "Citation source is missing or its canonical identity is invalid",
    )?;
    require(row.get(5)?, "Citation entity label identity is invalid")?;
    let bytes = row.get::<_, usize>(6)?;
    require(
        bytes <= MAX_CITATION_PROJECTION_BYTES,
        "Citation projection exceeds the 2 MiB byte bound",
    )?;
    let position = Position {
        kind_rank: row.get(0)?,
        sequence: row.get(1)?,
    };
    require(
        position.sequence > 0,
        "Canonical citation sequence is invalid",
    )?;
    Ok(Metadata {
        position,
        id: key.into(),
        bytes,
    })
}
fn decode(bytes: &[u8], entry: &Metadata) -> Result<CitationSummary> {
    require(
        bytes.len() == entry.bytes && bytes.len() <= MAX_CITATION_PROJECTION_BYTES,
        "Citation projection size changed",
    )?;
    let row: CitationSummary = serde_json::from_slice(bytes)?;
    require(
        row.id() == entry.id,
        "Canonical citation body identity is invalid",
    )?;
    let kind_rank = match row.kind() {
        CitationKind::Observation => 0,
        CitationKind::Transaction => 1,
        CitationKind::Evidence => 2,
    };
    require(
        kind_rank == entry.position.kind_rank,
        "Canonical citation kind changed",
    )?;
    row.validate()?;
    Ok(row)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    schema_version: u32,
    query_sha256: String,
    position: Position,
}
impl Cursor {
    fn encode(&self) -> Result<String> {
        Ok(serde_json::to_vec(self)?
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }
    fn decode(value: &str, query: &str) -> Result<Self> {
        require(
            value.len() <= MAX_CITATION_CURSOR_BYTES
                && value.len().is_multiple_of(2)
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Invalid citation cursor encoding",
        )?;
        let bytes = (0..value.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&value[i..i + 2], 16)
                    .map_err(|_| Error::Validation("Invalid citation cursor".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let cursor: Self = serde_json::from_slice(&bytes)?;
        require(
            cursor.schema_version == 1
                && cursor.query_sha256 == query
                && cursor.position.kind_rank <= 2
                && cursor.position.sequence > 0,
            "Citation cursor belongs to another query or revision",
        )?;
        Ok(cursor)
    }
}
fn revision(workspace: &Workspace, expected: u64) -> Result<u64> {
    let current = workspace.revision()?;
    if current != expected {
        return Err(Error::Conflict(
            "Citation revision changed; refresh and restart selection".into(),
        ));
    }
    Ok(current)
}
impl Workspace {
    pub fn page_citation_catalogue(
        &self,
        request: &CitationCatalogueRequest,
        expected_revision: u64,
    ) -> Result<CitationCataloguePage> {
        request.validate()?;
        let snapshot = self.conn.unchecked_transaction()?;
        let workspace_revision = revision(self, expected_revision)?;
        let excluded: BTreeSet<_> = request.excluded_ids.iter().collect();
        let matching = LiteralMatching::default();
        let lowered = lower_query(&request.query);
        let query_sha256 = hash(&serde_json::to_vec(&(
            "citation-catalogue-v1",
            workspace_revision,
            &matching,
            &lowered,
            &excluded,
            request.page_size,
        ))?);
        let cursor = request
            .cursor
            .as_ref()
            .map(|v| Cursor::decode(v, &query_sha256))
            .transpose()?;
        let query = format!(
            "{} SELECT {META},projection FROM projected
            WHERE id NOT IN (SELECT value FROM json_each(?1)) ORDER BY kind_rank,sequence",
            projection_sql()
        );
        let mut statement = snapshot.prepare(&query)?;
        let exclusions = serde_json::to_string(&excluded)?;
        let mut candidates = statement.query([exclusions])?;
        let mut rows = Vec::new();
        let mut bytes = 0usize;
        let mut scope_count = 0u64;
        let mut cursor_found = cursor.is_none();
        let mut last = None;
        let mut more = false;
        while let Some(candidate) = candidates.next()? {
            let entry = metadata(candidate)?;
            let row = decode(text(candidate, 7)?.as_bytes(), &entry)?;
            if !matches_text(&row.search_text(), &lowered) {
                continue;
            }
            scope_count = scope_count
                .checked_add(1)
                .ok_or_else(|| Error::Validation("Citation count overflow".into()))?;
            if let Some(cursor) = &cursor {
                if entry.position == cursor.position {
                    cursor_found = true;
                }
                if entry.position <= cursor.position {
                    continue;
                }
            }
            if more
                || rows.len() == request.page_size as usize
                || bytes + entry.bytes > MAX_CITATION_PROJECTION_BYTES
            {
                more = true;
                continue;
            }
            bytes += entry.bytes;
            last = Some(entry.position);
            rows.push(row);
        }
        require(
            cursor_found,
            "Citation cursor no longer identifies a matching uncited record",
        )?;
        drop(candidates);
        drop(statement);
        let next_cursor = if more {
            Some(
                Cursor {
                    schema_version: 1,
                    query_sha256: query_sha256.clone(),
                    position: last.ok_or_else(|| {
                        Error::Validation("Citation continuation has no preceding row".into())
                    })?,
                }
                .encode()?,
            )
        } else {
            None
        };
        snapshot.commit()?;
        Ok(CitationCataloguePage {
            schema_version: 1,
            workspace_revision,
            matching,
            query_sha256,
            scope_count,
            rows,
            next_cursor,
        })
    }
    pub fn read_citation_selections(
        &self,
        request: &CitationSelectionsRequest,
        expected_revision: u64,
    ) -> Result<CitationSelections> {
        request.validate()?;
        let snapshot = self.conn.unchecked_transaction()?;
        let workspace_revision = revision(self, expected_revision)?;
        let base = projection_sql();
        let mut statement = snapshot.prepare(&format!(
            "{base} SELECT {META} FROM projected
            WHERE id IN (SELECT value FROM json_each(?1)) ORDER BY kind_rank,sequence"
        ))?;
        let requested = serde_json::to_string(&request.ids)?;
        let mut candidates = statement.query([requested])?;
        let mut entries = Vec::new();
        let mut total = 0usize;
        // Complete projected-size preflight before any selected body is decoded.
        while let Some(row) = candidates.next()? {
            let entry = metadata(row)?;
            total = total
                .checked_add(entry.bytes)
                .ok_or_else(|| Error::Validation("Citation selection size overflow".into()))?;
            require(total <= MAX_CITATION_PROJECTION_BYTES, "Selected citations exceed the 2 MiB projected byte bound; no partial result returned")?;
            entries.push(entry);
        }
        require(
            entries.len() == request.ids.len(),
            "A selected citation is missing or unsupported",
        )?;
        drop(candidates);
        drop(statement);
        let mut statement = snapshot.prepare(&format!(
            "{base} SELECT projection FROM projected WHERE sequence=?"
        ))?;
        let mut rows = Vec::with_capacity(entries.len());
        for entry in entries {
            let mut results = statement.query([entry.position.sequence])?;
            let value = results
                .next()?
                .ok_or_else(|| Error::Validation("Selected citation disappeared".into()))?;
            rows.push(decode(text(value, 0)?.as_bytes(), &entry)?);
        }
        drop(statement);
        snapshot.commit()?;
        Ok(CitationSelections {
            schema_version: 1,
            workspace_revision,
            rows,
        })
    }
}
#[cfg(test)]
#[path = "citation_catalogue_tests.rs"]
mod tests;
