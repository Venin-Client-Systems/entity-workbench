//! Fixed canonical queries with byte/count preflight before Rust materialization.
use super::*;
use rusqlite::Row;

pub(super) fn identifier(value: &str) -> Result<()> {
    require(
        !value.is_empty()
            && value.len() <= 128
            && value.trim() == value
            && !value.chars().any(char::is_control),
        "Graph canonical identifier is invalid",
    )
}

pub(super) fn revision(conn: &Connection) -> Result<u64> {
    Ok(conn.query_row("SELECT revision FROM meta", [], |row| row.get(0))?)
}

#[derive(Default)]
struct Budget {
    canonical_bytes: usize,
    original_bytes: u64,
    fingerprints: BTreeMap<(String, String), String>,
}
impl Budget {
    fn decode<T: DeserializeOwned>(
        &mut self,
        row: &Row<'_>,
        kind: &str,
        identity: impl FnOnce(&T) -> &str,
    ) -> Result<(String, T)> {
        // Borrow SQLite values, check their lengths, then decode. Do not allocate
        // an arbitrary-size String or JSON Value before enforcing these bounds.
        let key = row
            .get_ref(0)?
            .as_str()
            .map_err(|_| Error::Validation("Graph record key is not text".into()))?;
        identifier(key)?;
        let body = row
            .get_ref(1)?
            .as_str()
            .map_err(|_| Error::Validation("Graph record body is not text".into()))?;
        require(
            body.len() <= MAX_RECORD_BYTES,
            "Graph canonical record exceeds byte bound",
        )?;
        self.canonical_bytes = self
            .canonical_bytes
            .checked_add(body.len())
            .ok_or_else(|| Error::QuotaExhausted("Graph canonical byte count overflow".into()))?;
        require(
            self.canonical_bytes <= MAX_CANONICAL_BYTES,
            "Graph canonical aggregate exceeds byte bound",
        )?;
        let value: T = serde_json::from_str(body)?;
        require(
            identity(&value) == key,
            "Graph record lookup key differs from body identity",
        )?;
        require(
            self.fingerprints
                .insert((kind.into(), key.into()), hash(body.as_bytes()))
                .is_none(),
            "Duplicate graph canonical read",
        )?;
        Ok((key.into(), value))
    }
    fn many<T: DeserializeOwned>(
        &mut self,
        conn: &Connection,
        kind: &str,
        maximum: usize,
        identity: impl Fn(&T) -> &str,
    ) -> Result<BTreeMap<String, T>> {
        let count: u64 =
            conn.query_row("SELECT count(*) FROM records WHERE kind=?", [kind], |row| {
                row.get(0)
            })?;
        require(
            count <= maximum as u64,
            "Graph canonical row count exceeds bound",
        )?;
        let mut statement = conn.prepare("SELECT id,body FROM records WHERE kind=? ORDER BY id")?;
        let mut rows = statement.query([kind])?;
        let mut result = BTreeMap::new();
        while let Some(row) = rows.next()? {
            let (key, value) = self.decode(row, kind, &identity)?;
            require(
                result.len() < maximum && result.insert(key, value).is_none(),
                "Graph canonical row bound or duplicate",
            )?;
        }
        Ok(result)
    }
    fn one<T: DeserializeOwned>(
        &mut self,
        conn: &Connection,
        kind: &str,
        key: &str,
        identity: impl FnOnce(&T) -> &str,
    ) -> Result<T> {
        identifier(key)?;
        let mut statement = conn.prepare("SELECT id,body FROM records WHERE kind=? AND id=?")?;
        let mut rows = statement.query(params![kind, key])?;
        let row = rows
            .next()?
            .ok_or_else(|| Error::Validation("Graph provenance record is missing".into()))?;
        Ok(self.decode(row, kind, identity)?.1)
    }
}

fn text(value: &str, maximum: usize) -> Result<()> {
    require(
        !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control),
        "Graph canonical relationship metadata is invalid",
    )
}
fn validity(assertion: &Assertion) -> Result<()> {
    let date = |value: &str| -> Result<chrono::NaiveDate> {
        require(
            value.len() == 10
                && value.bytes().enumerate().all(|(i, c)| {
                    if i == 4 || i == 7 {
                        c == b'-'
                    } else {
                        c.is_ascii_digit()
                    }
                }),
            "Graph validity date is not ISO YYYY-MM-DD",
        )?;
        chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| Error::Validation("Graph validity date is invalid".into()))
    };
    let from = assertion.valid_from.as_deref().map(date).transpose()?;
    let through = assertion.valid_to.as_deref().map(date).transpose()?;
    require(
        !matches!((from,through),(Some(a),Some(b)) if a>b),
        "Graph validity interval is reversed",
    )
}

pub(super) fn read(
    root: &Path,
    conn: &Connection,
    source: &str,
    target: &str,
) -> Result<Selection> {
    let mut budget = Budget::default();
    let entities = budget.many(conn, "entity", MAX_NODES, |entity: &Entity| {
        entity.id.as_str()
    })?;
    let assertions = budget.many(
        conn,
        "assertion",
        MAX_ASSERTIONS,
        |assertion: &Assertion| assertion.id.as_str(),
    )?;
    let nodes: BTreeSet<String> = entities
        .iter()
        .filter(|(_, entity)| entity.merged_into.is_none())
        .map(|(id, _)| id.clone())
        .collect();
    require(
        nodes.contains(source) && nodes.contains(target),
        "Graph endpoints must be active canonical entities",
    )?;
    let mut edges: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    let mut selected_assertions = BTreeMap::new();
    let mut observations = BTreeMap::new();
    let mut evidence = BTreeMap::new();
    let mut reviews = ReviewCounts::default();
    for (key, assertion) in assertions {
        reviews.record(&assertion.review);
        if assertion.review != ReviewState::Accepted {
            continue;
        }
        require(
            nodes.contains(&assertion.subject_id) && nodes.contains(&assertion.object_id),
            "Accepted graph assertion has a missing or merged endpoint",
        )?;
        text(&assertion.predicate, 300)?;
        text(&assertion.confidence, 300)?;
        validity(&assertion)?;
        require(
            !assertion.observation_ids.is_empty()
                && assertion.observation_ids.len() <= 50
                && assertion
                    .observation_ids
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
                    == assertion.observation_ids.len(),
            "Accepted graph assertion requires unique bounded source observations",
        )?;
        for observation_id in &assertion.observation_ids {
            if observations.contains_key(observation_id) {
                continue;
            }
            require(
                observations.len() < MAX_OBSERVATIONS,
                "Graph source observation count exceeds bound",
            )?;
            let observation = budget.one(
                conn,
                "observation",
                observation_id,
                |value: &Observation| value.id.as_str(),
            )?;
            require(
                observation.review == ReviewState::Accepted
                    && nodes.contains(&observation.entity_id),
                "Graph source observation is unaccepted or has a missing/merged entity",
            )?;
            if !matches!(observation.anchor, SourceAnchor::Text { .. }) {
                return Err(Error::Blocked("Graph provenance currently supports text anchors only; selected accepted edges were not dropped".into()));
            }
            let source_id = observation.anchor.evidence_id();
            if !evidence.contains_key(source_id) {
                require(
                    evidence.len() < MAX_EVIDENCE,
                    "Graph evidence count exceeds bound",
                )?;
                let source = budget.one(conn, "evidence", source_id, |value: &Evidence| {
                    value.id.as_str()
                })?;
                originals::validate_reference(&source)?;
                budget.original_bytes = budget
                    .original_bytes
                    .checked_add(source.bytes)
                    .ok_or_else(|| {
                        Error::QuotaExhausted("Graph original byte count overflow".into())
                    })?;
                require(
                    budget.original_bytes <= MAX_ORIGINAL_BYTES,
                    "Graph referenced original aggregate exceeds bound",
                )?;
                // Reuse the authoritative no-follow, single-link, identity/hash-bound reader.
                read_original(root, &source)?;
                // Store the bounded source only within this read, for existing anchor validation.
                evidence.insert(source.id.clone(), source);
            }
            // The bounded source was already decoded/identity checked in the same read transaction.
            // Existing anchor validation provides canonical line-range semantics.
            identity::validate_anchor(conn, &observation.anchor)?;
            observations.insert(observation.id.clone(), observation);
        }
        edges
            .entry(edge_key(&assertion.subject_id, &assertion.object_id))
            .or_default()
            .push(key.clone());
        selected_assertions.insert(key, assertion);
    }
    let evidence = evidence
        .into_iter()
        .map(|(id, value)| {
            (
                id,
                EvidenceProvenance {
                    sha256: value.sha256.clone(),
                    bytes: value.bytes,
                    origin_group: value.origin_group.clone(),
                    frozen: value,
                },
            )
        })
        .collect();
    Ok(Selection {
        entities,
        nodes,
        edges,
        fingerprints: budget.fingerprints,
        assertions: selected_assertions,
        observations,
        evidence,
        assertion_reviews: reviews,
    })
}
