//! Analyst-authored identity records and comparisons. All writes use the canonical transaction.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn bounded_text(value: &str, limit: usize, label: &str) -> Result<()> {
    require(
        !value.is_empty() && value.len() <= limit && value.trim() == value
            && !value.chars().any(char::is_control),
        &format!("{label} must be trimmed, nonempty text of at most {limit} bytes without control characters"),
    )
}

fn validate_entity(input: &EntityInput) -> Result<()> {
    bounded_text(&input.name, 300, "Entity name")?;
    require(
        input.identifiers.len() <= 50,
        "At most 50 identifiers per entity",
    )?;
    let mut seen = BTreeSet::new();
    for identifier in &input.identifiers {
        bounded_text(&identifier.namespace, 100, "Identifier namespace")?;
        bounded_text(&identifier.value, 300, "Identifier value")?;
        require(
            seen.insert((&identifier.namespace, &identifier.value)),
            "Duplicate identifier on this entity",
        )?;
    }
    Ok(())
}

fn validate_anchor(conn: &Connection, anchor: &SourceAnchor) -> Result<()> {
    let e: Evidence = get(conn, "evidence", anchor.evidence_id())?;
    match anchor {
        SourceAnchor::Text { line_start, line_end, .. } => {
            let text = e.text.as_deref().ok_or_else(|| Error::Validation("Source has no text derivative".into()))?;
            require(*line_start > 0 && line_end >= line_start && *line_end as usize <= text.lines().count(), "Source line range is outside the retained text derivative")
        }
        SourceAnchor::Cell { sheet, row, column, .. } => {
            require(e.media_type == "text/csv" && sheet == "CSV" && *row >= 2, "Cell anchors currently require an imported CSV data row")?;
            let text = e.text.as_deref().ok_or_else(|| Error::Validation("CSV source has no text".into()))?;
            let mut reader = csv::Reader::from_reader(text.as_bytes());
            require(reader.headers()?.iter().any(|h| h == column), "Source column does not exist")?;
            let record = reader.records().nth((*row - 2) as usize).transpose()?;
            require(record.is_some(), "Source row does not exist")
        }
        _ => Err(Error::Blocked("Page, message and capture anchors require a verified extraction manifest; use an available text or CSV anchor".into())),
    }
}

fn invalidate_assertions(conn: &Connection, observation_id: &str) -> Result<()> {
    for mut assertion in all::<Assertion>(conn, "assertion")? {
        if assertion
            .observation_ids
            .iter()
            .any(|id| id == observation_id)
        {
            assertion.review = ReviewState::Pending;
            put(conn, "assertion", &assertion.id, &assertion)?;
        }
    }
    Ok(())
}

impl Workspace {
    pub fn inspect_source(&self, anchor: &SourceAnchor) -> Result<SourceExcerpt> {
        let tx = self.conn.unchecked_transaction()?;
        validate_anchor(&tx, anchor)?;
        let e: Evidence = get(&tx, "evidence", anchor.evidence_id())?;
        let text = e.text.as_deref().unwrap_or_default();
        let (location, quote) = match anchor {
            SourceAnchor::Text {
                line_start,
                line_end,
                ..
            } => (
                format!("Text lines {line_start}–{line_end}"),
                text.lines()
                    .skip((*line_start - 1) as usize)
                    .take((line_end - line_start + 1) as usize)
                    .flat_map(|line| line.chars().chain(std::iter::once('\n')))
                    .take(8001)
                    .collect::<String>(),
            ),
            SourceAnchor::Cell {
                sheet, row, column, ..
            } => {
                let mut reader = csv::Reader::from_reader(text.as_bytes());
                let column_index = reader
                    .headers()?
                    .iter()
                    .position(|h| h == column)
                    .ok_or_else(|| Error::Validation("Source column disappeared".into()))?;
                let record = reader
                    .records()
                    .nth((*row - 2) as usize)
                    .transpose()?
                    .ok_or_else(|| Error::Validation("Source row disappeared".into()))?;
                (
                    format!("{sheet}, row {row}, column {column}"),
                    record[column_index].chars().take(8001).collect(),
                )
            }
            _ => {
                return Err(Error::Blocked(
                    "Verified source excerpt is unavailable".into(),
                ))
            }
        };
        let truncated = quote.chars().count() > 8000;
        let revision = tx.query_row("SELECT revision FROM meta", [], |r| r.get(0))?;
        Ok(SourceExcerpt {
            evidence_id: e.id,
            workspace_revision: revision,
            location,
            quote: quote.chars().take(8000).collect(),
            truncated,
        })
    }
    pub fn add_entity(&mut self, input: EntityInput, why: &str, expected: u64) -> Result<String> {
        validate_entity(&input)?;
        reason(why)?;
        let key = id();
        self.change(Some(expected), "entity.add", true, |conn| {
            let entity = Entity {
                id: key.clone(),
                name: input.name,
                kind: input.kind,
                identifiers: input.identifiers,
                merged_into: None,
            };
            put(conn, "entity", &key, &entity)?;
            record_decision(conn, &key, ReviewState::Pending, why)
        })?;
        Ok(key)
    }

    pub fn update_entity(
        &mut self,
        key: &str,
        input: EntityInput,
        why: &str,
        expected: u64,
    ) -> Result<()> {
        validate_entity(&input)?;
        reason(why)?;
        self.change(Some(expected), "entity.update", true, |conn| {
            let mut entity: Entity = get(conn, "entity", key)?;
            require(
                entity.merged_into.is_none(),
                "Reverse this entity's merge before editing its record",
            )?;
            require(
                !all::<Entity>(conn, "entity")?.iter().any(|other| {
                    other.merged_into.as_deref() == Some(key) && other.kind != input.kind
                }),
                "Reverse dependent merges before changing the entity kind",
            )?;
            entity.name = input.name;
            entity.kind = input.kind;
            entity.identifiers = input.identifiers;
            put(conn, "entity", key, &entity)?;
            record_decision(conn, key, ReviewState::Pending, why)
        })
    }

    pub fn add_observation(
        &mut self,
        input: ObservationInput,
        why: &str,
        expected: u64,
    ) -> Result<String> {
        bounded_text(&input.field, 100, "Observation field")?;
        bounded_text(&input.value, 4000, "Observation value")?;
        reason(why)?;
        let key = id();
        self.change(Some(expected), "observation.add", true, |conn| {
            let _: Entity = get(conn, "entity", &input.entity_id)?;
            validate_anchor(conn, &input.anchor)?;
            let observation = Observation {
                id: key.clone(),
                entity_id: input.entity_id,
                field: input.field,
                value: input.value,
                anchor: input.anchor,
                extraction_quality: None,
                review: ReviewState::Pending,
            };
            put(conn, "observation", &key, &observation)?;
            record_decision(conn, &key, ReviewState::Pending, why)
        })?;
        Ok(key)
    }

    pub fn correct_observation(
        &mut self,
        key: &str,
        value: &str,
        anchor: SourceAnchor,
        why: &str,
        expected: u64,
    ) -> Result<()> {
        bounded_text(value, 4000, "Observation value")?;
        reason(why)?;
        self.change(Some(expected), "observation.correct", true, |conn| {
            let mut observation: Observation = get(conn, "observation", key)?;
            validate_anchor(conn, &anchor)?;
            observation.value = value.into();
            observation.anchor = anchor;
            observation.review = ReviewState::Pending;
            observation.extraction_quality = None;
            put(conn, "observation", key, &observation)?;
            invalidate_assertions(conn, key)?;
            record_decision(conn, key, ReviewState::Pending, why)
        })
    }

    pub fn review_observation(
        &mut self,
        key: &str,
        state: ReviewState,
        why: &str,
        expected: u64,
    ) -> Result<()> {
        reason(why)?;
        self.change(Some(expected), "observation.review", true, |conn| {
            let mut observation: Observation = get(conn, "observation", key)?;
            validate_anchor(conn, &observation.anchor)?;
            observation.review = state.clone();
            put(conn, "observation", key, &observation)?;
            invalidate_assertions(conn, key)?;
            record_decision(conn, key, state, why)
        })
    }

    pub fn decide_identity(
        &mut self,
        left: &str,
        right: &str,
        outcome: IdentityOutcome,
        why: &str,
        expected: u64,
    ) -> Result<()> {
        reason(why)?;
        require(left != right, "Compare two different entity records")?;
        self.change(Some(expected), "identity.decide", true, |conn| {
            let a: Entity = get(conn, "entity", left)?;
            let b: Entity = get(conn, "entity", right)?;
            require(
                a.merged_into.is_none() && b.merged_into.is_none(),
                "Reverse active merges before recording a separate-identity decision",
            )?;
            let decision = IdentityDecision {
                id: id(),
                left_id: left.into(),
                right_id: right.into(),
                outcome,
                reason: why.into(),
                at: now(),
            };
            put(conn, "identity_decision", &decision.id, &decision)
        })
    }

    pub fn compare_entities(&self, left: &str, right: &str) -> Result<IdentityComparison> {
        require(left != right, "Compare two different entity records")?;
        // Read entity, observations, source groups and revision from one snapshot.
        let tx = self.conn.unchecked_transaction()?;
        let a: Entity = get(&tx, "entity", left)?;
        let b: Entity = get(&tx, "entity", right)?;
        let evidence: BTreeMap<_, _> = all::<Evidence>(&tx, "evidence")?
            .into_iter()
            .map(|e| (e.id, e.origin_group))
            .collect();
        let mut fields: BTreeMap<String, ComparisonField> = BTreeMap::new();
        for observation in all::<Observation>(&tx, "observation")?
            .into_iter()
            .filter(|o| o.entity_id == left || o.entity_id == right)
        {
            let field =
                fields
                    .entry(observation.field.clone())
                    .or_insert_with(|| ComparisonField {
                        field: observation.field.clone(),
                        left: vec![],
                        right: vec![],
                        signal: ComparisonSignal::InsufficientReviewedEvidence,
                        source_groups: vec![],
                    });
            if observation.entity_id == left {
                field.left.push(observation);
            } else {
                field.right.push(observation);
            }
        }
        for field in fields.values_mut() {
            let accepted = |rows: &Vec<Observation>| {
                rows.iter()
                    .filter(|o| o.review == ReviewState::Accepted)
                    .map(|o| o.value.clone())
                    .collect::<BTreeSet<_>>()
            };
            let a = accepted(&field.left);
            let b = accepted(&field.right);
            field.signal = if a.is_empty() || b.is_empty() {
                ComparisonSignal::InsufficientReviewedEvidence
            } else if a == b {
                ComparisonSignal::SharedReviewedValues
            } else if a.is_disjoint(&b) {
                ComparisonSignal::DifferentReviewedValues
            } else {
                ComparisonSignal::MixedReviewedValues
            };
            field.source_groups = field
                .left
                .iter()
                .chain(&field.right)
                .filter(|o| o.review == ReviewState::Accepted)
                .filter_map(|o| evidence.get(o.anchor.evidence_id()).cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
        }
        let revision = tx.query_row("SELECT revision FROM meta", [], |r| r.get(0))?;
        Ok(IdentityComparison {
            workspace_revision: revision,
            left: a,
            right: b,
            fields: fields.into_values().collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn correcting_supporting_observation_reopens_reviewed_relationship() {
        let temp = tempfile::TempDir::new_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut w = Workspace::open(temp.path().join("case")).unwrap();
        w.seed_demo().unwrap();
        // Fixture establishes an already reviewed relationship; correction must invalidate it.
        let mut assertion: Assertion = get(&w.conn, "assertion", "relationship-a").unwrap();
        assertion.review = ReviewState::Accepted;
        put(&w.conn, "assertion", &assertion.id, &assertion).unwrap();
        let observation: Observation = get(&w.conn, "observation", "obs-c").unwrap();
        w.correct_observation(
            &observation.id,
            "Notice corrected",
            observation.anchor,
            "Source correction",
            w.revision().unwrap(),
        )
        .unwrap();
        assert_eq!(w.view().unwrap().assertions[0].review, ReviewState::Pending);
    }
}
