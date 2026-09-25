//! Assessment authoring, citation checks and explicit analyst review.
use super::*;
use std::collections::BTreeSet;

fn text(value: &str, limit: usize, label: &str) -> Result<()> {
    require(
        !value.trim().is_empty()
            && value.len() <= limit
            && !value
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t'),
        &format!("{label} must contain 1 to {limit} bytes of text"),
    )
}
fn text_list(values: &[String], label: &str) -> Result<()> {
    require(values.len() <= 30, &format!("At most 30 {label}"))?;
    let mut seen = BTreeSet::new();
    for value in values {
        text(value, 2000, label)?;
        require(seen.insert(value.trim()), &format!("Duplicate {label}"))?;
    }
    Ok(())
}
fn validate_finding(conn: &Connection, input: &FindingInput) -> Result<()> {
    text(&input.title, 300, "Finding title")?;
    text(&input.assessment, 12_000, "Assessment")?;
    text(&input.limitations, 6000, "Limitations")?;
    require(
        !input.supporting_ids.is_empty() || !input.contradicting_ids.is_empty(),
        "Cite at least one evidence item, observation or transaction",
    )?;
    require(
        input.supporting_ids.len() + input.contradicting_ids.len() <= 100,
        "At most 100 citations per finding",
    )?;
    let mut seen = BTreeSet::new();
    for key in input.supporting_ids.iter().chain(&input.contradicting_ids) {
        require(seen.insert(key), "A record can be cited only once in a finding; use separate observations for different claims")?;
        let count: u32 = conn.query_row("SELECT count(*) FROM records WHERE id=? AND kind IN ('evidence','observation','transaction')", [key], |r| r.get(0))?;
        require(count == 1, "Finding citation is unresolved")?;
    }
    require(
        input.hypothesis_ids.len() <= 30,
        "At most 30 linked questions",
    )?;
    seen.clear();
    for key in &input.hypothesis_ids {
        require(seen.insert(key), "Duplicate question link")?;
        get::<Hypothesis>(conn, "hypothesis", key)?;
    }
    Ok(())
}
fn finding(key: String, input: FindingInput) -> Finding {
    Finding {
        id: key,
        hypothesis_ids: input.hypothesis_ids,
        title: input.title,
        assessment: input.assessment,
        supporting_ids: input.supporting_ids,
        contradicting_ids: input.contradicting_ids,
        limitations: input.limitations,
        needs_review: true,
    }
}
impl Workspace {
    pub fn save_question(
        &mut self,
        key: Option<&str>,
        input: HypothesisInput,
        why: &str,
        expected: u64,
    ) -> Result<String> {
        text(&input.question, 1000, "Investigation question")?;
        text(&input.proposition, 6000, "Working hypothesis")?;
        text_list(&input.alternatives, "alternative explanations")?;
        text_list(&input.gaps, "collection gaps")?;
        reason(why)?;
        let id = key.map(str::to_owned).unwrap_or_else(id);
        self.change(Some(expected), "question.save", true, |conn| {
            if let Some(key) = key {
                get::<Hypothesis>(conn, "hypothesis", key)?;
            }
            let question = Hypothesis {
                id: id.clone(),
                question: input.question,
                proposition: input.proposition,
                alternatives: input.alternatives,
                gaps: input.gaps,
            };
            put(conn, "hypothesis", &id, &question)?;
            record_decision(conn, &id, ReviewState::Pending, why)
        })?;
        Ok(id)
    }
    pub fn add_finding(&mut self, input: FindingInput, expected: u64) -> Result<String> {
        let key = id();
        self.change(Some(expected), "finding.add", false, |conn| {
            validate_finding(conn, &input)?;
            put(conn, "finding", &key, &finding(key.clone(), input))
        })?;
        Ok(key)
    }
    pub fn update_finding(
        &mut self,
        key: &str,
        input: FindingInput,
        why: &str,
        expected: u64,
    ) -> Result<()> {
        reason(why)?;
        self.change(Some(expected), "finding.update", false, |conn| {
            get::<Finding>(conn, "finding", key)?;
            validate_finding(conn, &input)?;
            put(conn, "finding", key, &finding(key.into(), input))?;
            record_decision(conn, key, ReviewState::Pending, why)
        })
    }
    pub fn review_finding(&mut self, key: &str, why: &str, expected: u64) -> Result<()> {
        reason(why)?;
        let root = self.root.clone();
        self.change(Some(expected), "finding.review", false, |conn| {
            let mut f: Finding = get(conn, "finding", key)?;
            require(f.needs_review, "Finding already has a current review")?;
            let input = FindingInput {
                title: f.title.clone(),
                assessment: f.assessment.clone(),
                supporting_ids: f.supporting_ids.clone(),
                contradicting_ids: f.contradicting_ids.clone(),
                limitations: f.limitations.clone(),
                hypothesis_ids: f.hypothesis_ids.clone(),
            };
            validate_finding(conn, &input)?;
            // Citation role does not change the source record's own review state.
            // Both supporting and contradictory extracted records must be accepted.
            let mut originals = BTreeSet::new();
            for key in f.supporting_ids.iter().chain(&f.contradicting_ids) {
                let (kind, body): (String, String) = conn.query_row("SELECT kind, body FROM records WHERE id=? AND kind IN ('evidence','observation','transaction')", [key], |r| Ok((r.get(0)?, r.get(1)?)))?;
                let evidence_id = match kind.as_str() {
                    "observation" => {
                        let o: Observation = serde_json::from_str(&body)?;
                        require(o.review == ReviewState::Accepted, "Accept all cited observations and transactions before reviewing the finding")?;
                        super::identity::validate_anchor(conn, &o.anchor)?;
                        o.anchor.evidence_id().to_owned()
                    }
                    "transaction" => {
                        let t: Transaction = serde_json::from_str(&body)?;
                        require(t.review == ReviewState::Accepted, "Accept all cited observations and transactions before reviewing the finding")?;
                        super::identity::validate_anchor(conn, &t.anchor)?;
                        t.anchor.evidence_id().to_owned()
                    }
                    "evidence" => key.clone(),
                    _ => return Err(Error::Validation("Unsupported finding citation".into())),
                };
                originals.insert(evidence_id);
            }
            for id in originals {
                verify_original(&root, &get_evidence(conn, &id)?)?;
            }
            f.needs_review = false;
            put(conn, "finding", key, &f)?;
            record_decision(conn, key, ReviewState::Accepted, why)
        })
    }
}
