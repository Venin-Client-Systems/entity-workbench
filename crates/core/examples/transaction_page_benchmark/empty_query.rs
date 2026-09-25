//! Diagnostic of the two fixed empty-account SQL scans in the baseline paging implementation.
//! These copied statements are read-only profiling probes, not alternate application queries.
use super::*;
use rusqlite::{params, Connection, OpenFlags};

pub fn run(directory: &Path) -> Result<()> {
    let connection = Connection::open_with_flags(
        directory.join("workspace.db"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let scope = "kind='transaction'
        AND (?1 IS NULL OR json_extract(body,'$.date') >= ?1)
        AND (?2 IS NULL OR json_extract(body,'$.date') <= ?2)
        AND (?3 IS NULL OR json_extract(body,'$.account') = ?3)
        AND (?4 IS NULL OR json_extract(body,'$.currency') = ?4)";
    let count = format!(
        "SELECT CASE json_extract(body,'$.review')
        WHEN 'accepted' THEN 'accepted' WHEN 'pending' THEN 'pending'
        WHEN 'rejected' THEN 'rejected' WHEN 'deferred' THEN 'deferred'
        ELSE 'invalid' END AS review_state, count(*) FROM records
        WHERE {scope} GROUP BY review_state"
    );
    let candidates = format!(
        "SELECT sequence,id,length(CAST(body AS BLOB)),length(CAST(id AS BLOB))
        FROM records WHERE {scope} AND (?5 IS NULL OR json_extract(body,'$.review') = ?5)
        AND (?6 IS NULL OR json_extract(body,'$.date') > ?6
            OR (json_extract(body,'$.date')=?6 AND sequence>?7))
        ORDER BY json_extract(body,'$.date') ASC, sequence ASC LIMIT ?8"
    );
    for (name, sql) in [
        ("empty_count_scan", count),
        ("empty_candidate_scan", candidates),
    ] {
        let mut plan = connection.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
        let bind = |statement: &mut rusqlite::Statement<'_>| -> rusqlite::Result<()> {
            statement.raw_bind_parameter(1, Option::<String>::None)?;
            statement.raw_bind_parameter(2, Option::<String>::None)?;
            statement.raw_bind_parameter(3, "SYNTHETIC-NONEXISTENT")?;
            statement.raw_bind_parameter(4, Option::<String>::None)?;
            if name == "empty_candidate_scan" {
                statement.raw_bind_parameter(5, Option::<String>::None)?;
                statement.raw_bind_parameter(6, Option::<String>::None)?;
                statement.raw_bind_parameter(7, Option::<i64>::None)?;
                statement.raw_bind_parameter(8, 201)?;
            }
            Ok(())
        };
        bind(&mut plan)?;
        let mut plan_rows = plan.raw_query();
        let mut details = Vec::new();
        while let Some(row) = plan_rows.next()? {
            details.push(row.get::<_, String>(3)?);
        }
        emit(
            json!({"event":"query_plan", "phase":name, "sqlite_version":rusqlite::version(),
            "query_sha256":format!("{:x}", Sha256::digest(sql.as_bytes())), "details":details,
            "diagnostic_only":true}),
        );
        for sample in 0..SAMPLES {
            let started = Instant::now();
            let mut statement = connection.prepare(&sql)?;
            bind(&mut statement)?;
            let mut rows = statement.raw_query();
            let mut count = 0;
            while rows.next()?.is_some() {
                count += 1;
            }
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            require(count == 0, "Empty diagnostic unexpectedly selected rows")?;
            emit(json!({"event":"query_phase", "phase":name, "sample":sample,
                "elapsed_ms":elapsed_ms, "rows":count, "diagnostic_only":true}));
        }
    }
    let revision: u64 =
        connection.query_row("SELECT revision FROM meta", params![], |row| row.get(0))?;
    emit(
        json!({"event":"query_diagnostic_complete", "samples_per_phase":SAMPLES,
        "workspace_revision":revision, "diagnostic_only":true}),
    );
    Ok(())
}
