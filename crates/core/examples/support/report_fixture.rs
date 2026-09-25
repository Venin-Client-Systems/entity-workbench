//! Fixed synthetic adapter fixture. These records are not canonical publication evidence.
use sha2::{Digest, Sha256};
use workbench_core::domain::*;

pub fn fixture() -> WorkspaceView {
    let text = "Synthetic source register\nCafé North appears in the copied source.\nA second account has a conflicting description.\n";
    let sha = format!("{:x}", Sha256::digest(text.as_bytes()));
    let source = Evidence {
        id: sha.clone(),
        sha256: sha.clone(),
        name: "Synthetic research notes.txt".into(),
        bytes: text.len() as u64,
        media_type: "text/plain".into(),
        origin_group: "copied-source-family-01".into(),
        imported_at: "2025-03-10T00:00:00Z".into(),
        extraction_status: "complete".into(),
        text: Some(text.into()),
        acquisitions: vec![Acquisition {
            job_id: "synthetic-collection".into(),
            url: "https://example.com/synthetic?x=1&y=2".into(),
            retrieved_at: "2025-03-09T00:00:00Z".into(),
        }],
    };
    let entity = Entity {
        id: "entity-a".into(),
        name: "Synthetic Café North".into(),
        kind: EntityKind::Organisation,
        identifiers: vec![Identifier {
            namespace: "Reference Number".into(),
            value: "000047".into(),
        }],
        merged_into: None,
    };
    let anchor = SourceAnchor::Text {
        evidence_id: sha.clone(),
        line_start: 2,
        line_end: 3,
    };
    let observation = Observation {
        id: "observation-a".into(),
        entity_id: entity.id.clone(),
        field: "Registered name".into(),
        value: "Café North & Partners".into(),
        anchor: anchor.clone(),
        extraction_quality: Some(0.82),
        review: ReviewState::Accepted,
    };
    let mut transactions = Vec::new();
    for index in 0..18 {
        let amount = match index {
            0 => "100.00",
            3 => "-20.00",
            4 => "20.00",
            5 => "3.00",
            _ => "-1.23000001",
        };
        let description = match index {
            1 => "<script>literal hostile text</script> & café purchase".into(),
            2 => "Repeated purchase\nSecond line\tDetails retained".into(),
            _ => format!("Synthetic merchant record {index:02} with an intentionally long description to exercise wrapping across the fixed table column."),
        };
        transactions.push(Transaction {
            id: format!("transaction-{index:02}"),
            account: if index == 4 { "0002" } else { "0001" }.into(),
            date: format!("2025-03-{:02}", index + 1),
            posting_date: Some(format!("2025-03-{:02}", index + 1)),
            description,
            amount: amount.into(),
            currency: if index == 6 { "USD" } else { "AUD" }.into(),
            balance: None,
            anchor: SourceAnchor::Cell {
                evidence_id: sha.clone(),
                sheet: "Retained synthetic table".into(),
                row: index + 2,
                column: "amount".into(),
            },
            review: match index {
                7 => ReviewState::Pending,
                8 => ReviewState::Rejected,
                9 => ReviewState::Deferred,
                _ => ReviewState::Accepted,
            },
            duplicate_candidates: if index == 1 {
                vec!["transaction-02".into()]
            } else {
                Vec::new()
            },
            transfer_peer: match index {
                3 => Some("transaction-04".into()),
                4 => Some("transaction-03".into()),
                _ => None,
            },
            merchant: None,
            version: 1,
        });
    }
    let question = Hypothesis {
        id: "question-a".into(),
        question: "Do the transactions identify the same organisation".into(),
        proposition:
            "The available records support further examination of a possible shared counterparty."
                .into(),
        alternatives: vec![
            "Namesakes or unrelated branches remain possible.".into(),
            "The source may have copied an earlier error.".into(),
        ],
        gaps: vec!["Obtain an independent source and review historical validity.".into()],
    };
    let finding = Finding {
        id: "finding-a".into(),
        hypothesis_ids: vec![question.id.clone()],
        title: "Counterparty identity remains unresolved".into(),
        assessment: "The name appears in a retained source and a reviewed transaction. The contrary transaction description prevents a final identity conclusion. Monetary totals below preserve separate currencies and explicit transfer treatment.\n\nThe copied source is not an independent confirmation.".into(),
        supporting_ids: vec![observation.id.clone(), "transaction-01".into()],
        contradicting_ids: vec!["transaction-02".into(), sha],
        limitations: "Synthetic demonstration only. Source anchors are preserved typed specimen values, not independently verified page or table locations.".into(),
        needs_review: true,
    };
    WorkspaceView {
        schema_version: 4,
        revision: 42,
        entities: vec![entity],
        evidence: vec![source],
        observations: vec![observation],
        assertions: vec![],
        transactions,
        addresses: vec![],
        locations: vec![],
        leads: vec![],
        jobs: vec![],
        findings: vec![finding],
        hypotheses: vec![question],
        decisions: vec![ReviewDecision {
            id: "review-a".into(),
            target_id: "finding-a".into(),
            state: ReviewState::Pending,
            reason: "Conflicting record remains unresolved.\nA literal =HYPERLINK(\"https://example.com\") is plain text.".into(),
            at: "2025-03-10T00:00:00Z".into(),
        }],
        merges: vec![],
        identity_decisions: vec![],
        reports: vec![],
        statement_profiles: vec![],
        statement_imports: vec![],
    }
}
