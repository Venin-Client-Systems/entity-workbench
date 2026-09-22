use tempfile::TempDir;
use workbench_core::{domain::*, store::Workspace};

fn workspace() -> (TempDir, Workspace) {
    let temp = TempDir::new_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let w = Workspace::open(temp.path().join("case")).unwrap();
    (temp, w)
}
fn person(name: &str) -> EntityInput {
    EntityInput {
        name: name.into(),
        kind: EntityKind::Person,
        identifiers: vec![Identifier {
            namespace: "CASE".into(),
            value: "000001".into(),
        }],
    }
}
fn pair(w: &mut Workspace) -> (String, String, String) {
    let a = w
        .add_entity(
            person("Avery Vale"),
            "First source identity",
            w.revision().unwrap(),
        )
        .unwrap();
    let b = w
        .add_entity(
            person("Avery Vale"),
            "Second source identity",
            w.revision().unwrap(),
        )
        .unwrap();
    let source = w
        .import(
            "identities.txt",
            b"Avery Vale born 1982\nAvery Vale born 1990\n",
        )
        .unwrap();
    (a, b, source)
}
fn observation(entity: &str, source: &str, value: &str, line: u32) -> ObservationInput {
    ObservationInput {
        entity_id: entity.into(),
        field: "birth_year".into(),
        value: value.into(),
        anchor: SourceAnchor::Text {
            evidence_id: source.into(),
            line_start: line,
            line_end: line,
        },
    }
}
#[test]
fn authoring_preserves_namesakes_namespaces_and_leading_zeros() {
    let (_temp, mut w) = workspace();
    let (a, b, _) = pair(&mut w);
    assert_ne!(a, b);
    let entities = w.view().unwrap().entities;
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[0].identifiers[0].value, "000001");
    assert_eq!(entities[1].identifiers[0].namespace, "CASE");
    for kind in [
        EntityKind::Organisation,
        EntityKind::Group,
        EntityKind::Account,
        EntityKind::Place,
        EntityKind::DigitalIdentifier,
    ] {
        let mut input = person("Synthetic record");
        input.kind = kind;
        w.add_entity(input, "Fixture record", w.revision().unwrap())
            .unwrap();
    }
    assert_eq!(w.view().unwrap().entities.len(), 7);
}
#[test]
fn invalid_or_stale_entity_edits_leave_no_partial_records() {
    let (_temp, mut w) = workspace();
    for name in ["", " spaced", "control\nname"] {
        assert!(w.add_entity(person(name), "Fixture", 0).is_err());
    }
    let mut duplicate = person("Valid name");
    duplicate.identifiers.push(duplicate.identifiers[0].clone());
    assert!(w.add_entity(duplicate, "Fixture", 0).is_err());
    assert_eq!(w.revision().unwrap(), 0);
    let id = w.add_entity(person("Valid name"), "Fixture", 0).unwrap();
    assert!(w
        .update_entity(&id, person("Stale update"), "Fixture", 0)
        .is_err());
    w.update_entity(&id, person("Corrected name"), "Source label corrected", 1)
        .unwrap();
    assert_eq!(w.view().unwrap().entities[0].name, "Corrected name");
    assert_eq!(w.view().unwrap().decisions.len(), 2);
}
#[test]
fn observations_cannot_cite_missing_entities_or_nonexistent_source_regions() {
    let (_temp, mut w) = workspace();
    let (a, _, source) = pair(&mut w);
    let revision = w.revision().unwrap();
    for input in [
        observation("missing", &source, "1982", 1),
        observation(&a, "missing", "1982", 1),
        observation(&a, &source, "1982", 0),
        observation(&a, &source, "1982", 3),
    ] {
        assert!(w
            .add_observation(input, "Source checked", revision)
            .is_err());
    }
    let mut unverified_page = observation(&a, &source, "1982", 1);
    unverified_page.anchor = SourceAnchor::Page {
        evidence_id: source,
        page: 1,
        region: None,
    };
    assert!(w
        .add_observation(unverified_page, "Source checked", revision)
        .is_err());
    assert_eq!(w.revision().unwrap(), revision);
    assert!(w.view().unwrap().observations.is_empty());
}
#[test]
fn csv_anchor_uses_logical_rows_even_with_embedded_newlines() {
    let (_temp, mut w) = workspace();
    let a = w.add_entity(person("Avery Vale"), "Fixture", 0).unwrap();
    let source = w.import("rows.csv", b"account,date,description,amount,currency\nA,2025-03-01,\"Line one\nline two\",-1.00,AUD\n").unwrap();
    let mut input = observation(&a, &source, "Line one / line two", 1);
    input.anchor = SourceAnchor::Cell {
        evidence_id: source.clone(),
        sheet: "CSV".into(),
        row: 2,
        column: "description".into(),
    };
    w.add_observation(
        input.clone(),
        "Statement cell inspected",
        w.revision().unwrap(),
    )
    .unwrap();
    let excerpt = w.inspect_source(&input.anchor).unwrap();
    assert_eq!(excerpt.quote, "Line one\nline two");
    assert_eq!(excerpt.location, "CSV, row 2, column description");
    for (row, column, sheet) in [
        (3, "description", "CSV"),
        (2, "missing", "CSV"),
        (2, "description", "Invented sheet"),
    ] {
        input.anchor = SourceAnchor::Cell {
            evidence_id: source.clone(),
            sheet: sheet.into(),
            row,
            column: column.into(),
        };
        assert!(w
            .add_observation(input.clone(), "Fixture", w.revision().unwrap())
            .is_err());
    }
    assert_eq!(w.view().unwrap().observations.len(), 1);
}
#[test]
fn comparisons_distinguish_reviewed_differences_from_unreviewed_claims() {
    let (_temp, mut w) = workspace();
    let (a, b, source) = pair(&mut w);
    let first = w
        .add_observation(
            observation(&a, &source, "1982", 1),
            "Fixture",
            w.revision().unwrap(),
        )
        .unwrap();
    let second = w
        .add_observation(
            observation(&b, &source, "1990", 2),
            "Fixture",
            w.revision().unwrap(),
        )
        .unwrap();
    let pending = w.compare_entities(&a, &b).unwrap();
    assert_eq!(
        pending.fields[0].signal,
        ComparisonSignal::InsufficientReviewedEvidence
    );
    assert!(pending.fields[0].source_groups.is_empty());
    for id in [&first, &second] {
        w.review_observation(
            id,
            ReviewState::Accepted,
            "Source inspected",
            w.revision().unwrap(),
        )
        .unwrap();
    }
    let compared = w.compare_entities(&a, &b).unwrap();
    assert_eq!(compared.workspace_revision, w.revision().unwrap());
    assert_eq!(
        compared.fields[0].signal,
        ComparisonSignal::DifferentReviewedValues
    );
    assert_eq!(
        compared.fields[0].source_groups.len(),
        1,
        "Two claims from one source are not two source groups"
    );
    assert!(compared.left.merged_into.is_none());
    assert_eq!(compared.fields[0].left[0].extraction_quality, None);
    let third = w
        .add_observation(
            observation(&b, &source, "1982", 1),
            "Alternative claim",
            w.revision().unwrap(),
        )
        .unwrap();
    w.review_observation(
        &third,
        ReviewState::Accepted,
        "Reviewed alternative",
        w.revision().unwrap(),
    )
    .unwrap();
    assert_eq!(
        w.compare_entities(&a, &b).unwrap().fields[0].signal,
        ComparisonSignal::MixedReviewedValues
    );
    w.review_observation(
        &second,
        ReviewState::Rejected,
        "Contradicted by source review",
        w.revision().unwrap(),
    )
    .unwrap();
    assert_eq!(
        w.compare_entities(&a, &b).unwrap().fields[0].signal,
        ComparisonSignal::SharedReviewedValues
    );
    assert_eq!(
        w.compare_entities(&a, &b).unwrap().fields[0].right.len(),
        2,
        "Rejected claims remain visible"
    );
}
#[test]
fn merge_and_separate_decisions_work_for_authored_records_and_survive_restore() {
    let (temp, mut w) = workspace();
    let (a, b, _) = pair(&mut w);
    for outcome in [IdentityOutcome::Defer, IdentityOutcome::KeepSeparate] {
        w.decide_identity(
            &a,
            &b,
            outcome,
            "More evidence required",
            w.revision().unwrap(),
        )
        .unwrap();
    }
    assert_eq!(w.view().unwrap().identity_decisions.len(), 2);
    let rev = w.revision().unwrap();
    assert!(w
        .decide_identity(&a, &a, IdentityOutcome::Defer, "Fixture", rev)
        .is_err());
    assert!(w
        .decide_identity(&a, &b, IdentityOutcome::Defer, "", rev)
        .is_err());
    assert!(w
        .decide_identity(&a, &b, IdentityOutcome::Defer, "Fixture", rev - 1)
        .is_err());
    w.merge(&a, &b, "Synthetic mistaken merge", rev).unwrap();
    let mut edited = person("Changed kind");
    edited.kind = EntityKind::Organisation;
    assert!(w
        .update_entity(&b, edited, "Fixture", w.revision().unwrap())
        .is_err());
    assert!(w
        .update_entity(&a, person("Changed name"), "Fixture", w.revision().unwrap())
        .is_err());
    assert!(w
        .decide_identity(
            &a,
            &b,
            IdentityOutcome::KeepSeparate,
            "Fixture",
            w.revision().unwrap()
        )
        .is_err());
    let merge = w.view().unwrap().merges[0].id.clone();
    w.reverse_merge(&merge, "Conflicting birth years", w.revision().unwrap())
        .unwrap();
    let backup = w.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored"))
        .unwrap()
        .view()
        .unwrap();
    assert_eq!(restored.identity_decisions.len(), 2);
    assert!(restored.merges[0].reversed);
    assert!(restored.entities.iter().all(|e| e.merged_into.is_none()));
}
#[test]
fn observation_correction_preserves_originals_and_previous_report() {
    let (_temp, mut w) = workspace();
    w.seed_demo().unwrap();
    w.save_report().unwrap();
    let before = w.view().unwrap();
    let o = &before.observations[0];
    w.review_observation(
        &o.id,
        ReviewState::Accepted,
        "Source checked",
        w.revision().unwrap(),
    )
    .unwrap();
    w.correct_observation(
        &o.id,
        "1985",
        o.anchor.clone(),
        "Synthetic correction",
        w.revision().unwrap(),
    )
    .unwrap();
    let after = w.view().unwrap();
    assert_eq!(after.observations[0].value, "1985");
    assert_eq!(after.observations[0].review, ReviewState::Pending);
    assert_eq!(after.reports[0].html, before.reports[0].html);
    assert_eq!(after.evidence[0].text, before.evidence[0].text);
    assert!(after.findings.iter().all(|f| f.needs_review));
}
#[test]
fn source_excerpt_is_bounded_and_invalid_ranges_fail_without_mutation() {
    let (_temp, mut w) = workspace();
    let text = format!("first\n{}\nlast", "é".repeat(9000));
    let id = w.import("long.txt", text.as_bytes()).unwrap();
    let anchor = SourceAnchor::Text {
        evidence_id: id.clone(),
        line_start: 2,
        line_end: 2,
    };
    let excerpt = w.inspect_source(&anchor).unwrap();
    assert_eq!(excerpt.quote.chars().count(), 8000);
    assert!(excerpt.truncated);
    assert_eq!(excerpt.workspace_revision, w.revision().unwrap());
    assert!(w
        .inspect_source(&SourceAnchor::Text {
            evidence_id: id,
            line_start: 3,
            line_end: 2
        })
        .is_err());
    assert_eq!(w.revision().unwrap(), 1);
}
