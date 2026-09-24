//! Metadata reads retain the canonical key instead of trusting a body's self-ID.
//! These checks do not read or hash original files; byte verification is separate.
use super::*;

fn decode(key: &str, body: &str) -> Result<Evidence> {
    let evidence: Evidence = serde_json::from_str(body)?;
    require(
        evidence.id == key,
        "Canonical evidence lookup key differs from its body identity",
    )?;
    originals::validate_reference(&evidence)?;
    Ok(evidence)
}

/// Missing is the only successful `None`; malformed or retargeted records fail.
pub(super) fn find_evidence(conn: &Connection, key: &str) -> Result<Option<Evidence>> {
    let body: Option<String> = conn
        .query_row(
            "SELECT body FROM records WHERE kind='evidence' AND id=?",
            [key],
            |row| row.get(0),
        )
        .optional()?;
    body.map(|body| decode(key, &body)).transpose()
}

pub(super) fn get_evidence(conn: &Connection, key: &str) -> Result<Evidence> {
    find_evidence(conn, key)?.ok_or_else(|| Error::Validation("Unknown evidence identifier".into()))
}

/// Preserve canonical sequence order and the caller's existing read snapshot.
pub(super) fn all_evidence(conn: &Connection) -> Result<Vec<Evidence>> {
    let mut statement =
        conn.prepare("SELECT id,body FROM records WHERE kind='evidence' ORDER BY sequence")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (key, body) = row?;
        decode(&key, &body)
    })
    .collect()
}
