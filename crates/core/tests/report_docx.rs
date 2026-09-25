#[path = "../examples/support/report_fixture.rs"]
mod fixture;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
};
use workbench_core::{
    analytics,
    domain::*,
    report,
    report_document::{self, ReportDocument, MAX_TEXT_BYTES},
    report_docx,
    store::Workspace,
};
const ID: &str = "11111111-2222-4333-8444-555555555555";
const AT: &str = "2025-03-10T12:00:00Z";
fn document() -> ReportDocument {
    report_document::capture(&fixture::fixture(), ID, AT).unwrap()
}
fn parts(bytes: &[u8]) -> BTreeMap<String, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut parts = BTreeMap::new();
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).unwrap();
        assert_eq!(file.compression(), zip::CompressionMethod::Stored);
        let mut xml = String::new();
        file.read_to_string(&mut xml).unwrap();
        assert!(parts.insert(file.name().into(), xml).is_none());
    }
    parts
}
#[test]
fn frozen_roundtrip_is_deterministic_editable_and_has_only_fixed_internal_parts() {
    let d = document();
    let bytes = report_docx::render(&d).unwrap();
    assert_eq!(
        bytes,
        report_docx::render(&ReportDocument::from_json(&d.to_json().unwrap()).unwrap()).unwrap()
    );
    let p = parts(&bytes);
    assert_eq!(
        p.keys().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from(
            [
                "[Content_Types].xml",
                "_rels/.rels",
                "docProps/core.xml",
                "word/document.xml",
                "word/styles.xml",
                "word/_rels/document.xml.rels"
            ]
            .map(String::from)
        )
    );
    for xml in p.values() {
        let mut reader = quick_xml::Reader::from_str(xml);
        while reader.read_event().unwrap() != quick_xml::events::Event::Eof {}
        for forbidden in [
            "TargetMode=",
            "altChunk",
            "vbaProject",
            "INCLUDETEXT",
            "<w:instrText",
            "attachedTemplate",
        ] {
            assert!(!xml.contains(forbidden));
        }
    }
    let main = &p["word/document.xml"];
    assert!(main.contains("&lt;script&gt;literal hostile text&lt;/script&gt;"));
    assert!(!main.contains("<script>"));
    assert!(main.contains("000047"));
    assert!(main.contains("copied-source-family-01"));
    assert!(main.contains("Supporting evidence"));
    assert!(main.contains("Contradicting evidence"));
    assert!(main.contains("<w:tblHeader/>"));
    assert!(main.contains("<w:pgSz w:w=\"12240\" w:h=\"15840\"/>"));
    assert!(main.contains("<w:tblW w:w=\"9360\" w:type=\"dxa\"/>"));
    assert!(p["word/styles.xml"].contains("<w:sz w:val=\"22\"/>"));
    assert!(p["word/styles.xml"].contains("<w:wordWrap w:val=\"on\"/>"));
    assert!(main.contains("<w:tab/>"));
    assert!(main.contains("<w:br/>"));
    assert!(p["word/styles.xml"].contains("w:styleId=\"Title\""));
    assert!(!p["word/styles.xml"].contains("themeColor"));
    let mut reader = quick_xml::Reader::from_str(main);
    let mut starts = BTreeSet::new();
    let mut ends = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut links = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            quick_xml::events::Event::Empty(e) if e.name().as_ref() == "w:bookmarkStart" => {
                let a: BTreeMap<_, _> = e
                    .attributes()
                    .map(|a| {
                        let a = a.unwrap();
                        (
                            a.key.as_ref().to_owned(),
                            a.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                                .unwrap()
                                .into_owned(),
                        )
                    })
                    .collect();
                assert!(starts.insert(a["w:id"].clone()));
                let name = &a["w:name"];
                assert!(name.len() <= 40);
                assert!(names.insert(name.clone()));
            }
            quick_xml::events::Event::Empty(e) if e.name().as_ref() == "w:bookmarkEnd" => {
                ends.insert(
                    e.attributes()
                        .next()
                        .unwrap()
                        .unwrap()
                        .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                        .unwrap()
                        .into_owned(),
                );
            }
            quick_xml::events::Event::Start(e) if e.name().as_ref() == "w:hyperlink" => {
                links.push(
                    e.attributes()
                        .next()
                        .unwrap()
                        .unwrap()
                        .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                        .unwrap()
                        .into_owned(),
                );
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    assert_eq!(starts, ends);
    assert!(links.iter().all(|target| names.contains(target)));
    assert_eq!(starts.len(), 24);
}
#[test]
fn shared_money_transfer_review_and_balance_semantics_are_frozen_without_recalculation_rules() {
    let mut view = fixture::fixture();
    view.transactions[0].balance = Some("100.00".into());
    view.transactions[1].balance = Some("98.76999999".into());
    let d = report_document::capture(&view, ID, AT).unwrap();
    let a = analytics::analyse(&view.transactions).unwrap();
    for (frozen, canonical) in d.calculations.totals.iter().zip(a.totals) {
        assert_eq!(frozen.currency, canonical.currency);
        assert_eq!(frozen.credits, canonical.credits);
        assert_eq!(frozen.debits, canonical.debits);
        assert_eq!(frozen.net, canonical.net);
        assert_eq!(frozen.transaction_ids, canonical.transaction_ids);
        assert_eq!(
            frozen.excluded_transfer_ids,
            canonical.excluded_transfer_ids
        );
    }
    assert_eq!(d.calculations.balances[0].difference, "0.00000000");
    assert!(d.calculations.balances[0].reconciled);
    assert_eq!(
        (
            d.calculations.accepted,
            d.calculations.pending,
            d.calculations.rejected,
            d.calculations.deferred
        ),
        (15, 1, 1, 1)
    );
    let mut corrupt = d.clone();
    corrupt.calculations.totals[0].net = "9999".into();
    assert!(report_docx::render(&corrupt).is_err());
    view.transactions[1].amount = "0.000000001".into();
    assert!(report_document::capture(&view, ID, AT).is_err());
}
#[test]
fn canonical_html_and_frozen_docx_survive_later_correction() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("case")).unwrap();
    w.seed_demo().unwrap();
    w.save_report().unwrap();
    let view = w.view().unwrap();
    let html = report::html(&view, ID).unwrap();
    let frozen = report_document::capture(&view, ID, AT).unwrap();
    let docx = report_docx::render(&frozen).unwrap();
    let saved = view.reports[0].clone();
    w.review_transaction(
        &view.transactions[0].id,
        ReviewState::Accepted,
        "Synthetic later source review",
        view.revision,
    )
    .unwrap();
    assert_eq!(report::html(&view, ID).unwrap(), html);
    assert_eq!(w.view().unwrap().reports[0].html, saved.html);
    assert_eq!(w.view().unwrap().reports[0].sha256, saved.sha256);
    assert_eq!(report_docx::render(&frozen).unwrap(), docx);
    assert_eq!(
        ReportDocument::from_json(&frozen.to_json().unwrap())
            .unwrap()
            .workspace_revision,
        view.revision
    );
}
#[test]
fn dangling_ambiguous_and_retargeted_sources_fail_closed() {
    for mutation in 0..7 {
        let mut view = fixture::fixture();
        match mutation {
            0 => view.findings[0].supporting_ids.push("missing".into()),
            1 => view.findings[0].hypothesis_ids.push("missing".into()),
            2 => view.observations[0].entity_id = "missing".into(),
            3 => {
                view.transactions[0].anchor = SourceAnchor::Text {
                    evidence_id: "missing".into(),
                    line_start: 1,
                    line_end: 1,
                }
            }
            4 => view.evidence[0].sha256 = "0".repeat(64),
            5 => view.entities[0].id = view.transactions[0].id.clone(),
            _ => view.transactions[0].transfer_peer = Some("missing".into()),
        }
        assert!(
            report_document::capture(&view, ID, AT).is_err(),
            "mutation {mutation}"
        );
    }
}
#[test]
fn invalid_xml_oversized_strings_nested_lists_rows_originals_and_serialized_input_are_rejected() {
    let mut view = fixture::fixture();
    view.findings[0].assessment.push('\0');
    assert!(report_document::capture(&view, ID, AT).is_err());
    let mut view = fixture::fixture();
    view.evidence[0].text = Some("x".repeat(MAX_TEXT_BYTES + 1));
    assert!(report_document::capture(&view, ID, AT).is_err());
    let mut view = fixture::fixture();
    view.hypotheses[0].alternatives = vec!["".into(); 10001];
    assert!(report_document::capture(&view, ID, AT).is_err());
    let mut view = fixture::fixture();
    view.transactions = vec![view.transactions[0].clone(); 5001];
    assert!(report_document::capture(&view, ID, AT).is_err());
    let mut view = fixture::fixture();
    view.evidence[0].bytes = 16 * 1024 * 1024 + 1;
    assert!(report_document::capture(&view, ID, AT).is_err());
    assert!(
        ReportDocument::from_json(&vec![b' '; report_document::MAX_DOCUMENT_BYTES + 1]).is_err()
    );
    let mut json = serde_json::to_value(document()).unwrap();
    json["unexpected"] = true.into();
    assert!(ReportDocument::from_json(&serde_json::to_vec(&json).unwrap()).is_err());
}
#[test]
fn aggregate_json_and_xml_expansion_limits_never_return_a_truncated_package() {
    let mut view = fixture::fixture();
    for n in 0..18 {
        view.entities.push(Entity {
            id: format!("entity-extra-{n}"),
            name: "x".repeat(MAX_TEXT_BYTES),
            kind: EntityKind::Group,
            identifiers: vec![],
            merged_into: None,
        });
    }
    assert!(report_document::capture(&view, ID, AT).is_err());
    view.entities.truncate(1);
    for n in 0..8 {
        view.entities.push(Entity {
            id: format!("entity-escaped-{n}"),
            name: "&".repeat(MAX_TEXT_BYTES),
            kind: EntityKind::Group,
            identifiers: vec![],
            merged_into: None,
        });
    }
    let frozen = report_document::capture(&view, ID, AT).unwrap();
    assert!(report_docx::render(&frozen).is_err());
}
#[test]
fn retained_anchor_variants_and_literal_whitespace_survive_frozen_roundtrip() {
    let mut view = fixture::fixture();
    let source = view.evidence[0].id.clone();
    view.findings[0].assessment = " leading\ttext\r\nnext line & <tag> trailing ".into();
    for (i, anchor) in [
        SourceAnchor::Page {
            evidence_id: source.clone(),
            page: 2,
            region: Some([0.1, 0.2, 0.3, 0.4]),
        },
        SourceAnchor::Message {
            evidence_id: source.clone(),
            message_id: "message-0001".into(),
        },
        SourceAnchor::Capture {
            evidence_id: source,
            selector: "#literal > content".into(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let mut o = view.observations[0].clone();
        o.id = format!("anchor-{i}");
        o.anchor = anchor;
        view.observations.push(o);
    }
    let d = report_document::capture(&view, ID, AT).unwrap();
    let copy = ReportDocument::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(
        copy.content.findings[0].assessment,
        view.findings[0].assessment
    );
    assert_eq!(
        serde_json::to_value(&copy.content.observations).unwrap(),
        serde_json::to_value(&view.observations).unwrap()
    );
    let main = &parts(&report_docx::render(&d).unwrap())["word/document.xml"];
    assert!(main.contains("&#13;"));
    assert!(main.contains("#literal &gt; content"));
}

#[test]
fn historical_generators_are_byte_stable_and_new_headers_keep_their_first_row() {
    use sha2::{Digest, Sha256};
    let legacy =
        ReportDocument::from_json(include_bytes!("fixtures/report-generator1.json")).unwrap();
    let old_bytes = report_docx::render(&legacy).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&old_bytes)),
        "c839120c7646195012e57119be34dcca7ad27e32a032ad734ca47680cfc1c75d"
    );
    assert_eq!(legacy.generator_version, "ooxml-foundation-1");
    let old_parts = parts(&old_bytes);
    assert_eq!(
        old_parts["word/styles.xml"]
            .matches("<w:wordWrap w:val=\"off\"/>")
            .count(),
        7
    );

    let current = document();
    assert_eq!(current.generator_version, "ooxml-foundation-3");
    let current_parts = parts(&report_docx::render(&current).unwrap());
    assert_eq!(
        current_parts["word/styles.xml"]
            .matches("<w:wordWrap w:val=\"on\"/>")
            .count(),
        7
    );
    assert!(!current_parts["word/styles.xml"].contains("<w:wordWrap w:val=\"off\"/>"));
    let second =
        ReportDocument::from_json(include_bytes!("fixtures/report-generator2.json")).unwrap();
    let second_bytes = report_docx::render(&second).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&second_bytes)),
        "0b9f69392b57dd1190dea7d45c7fcc16bad97ab9fcf2b9dec3b8350b6fc74fd9"
    );
    let second_parts = parts(&second_bytes);
    for (styles, keep_header) in [
        (&old_parts["word/styles.xml"], false),
        (&second_parts["word/styles.xml"], false),
        (&current_parts["word/styles.xml"], true),
    ] {
        let header = styles
            .split("w:styleId=\"TableHeader\"")
            .nth(1)
            .unwrap()
            .split("</w:style>")
            .next()
            .unwrap();
        assert_eq!(header.contains("<w:keepNext/>"), keep_header);
        let body = styles
            .split("w:styleId=\"Small\"")
            .nth(1)
            .unwrap()
            .split("</w:style>")
            .next()
            .unwrap();
        assert!(
            !body.contains("<w:keepNext/>"),
            "Data rows must remain free to paginate"
        );
    }
    let mut unsupported = legacy;
    unsupported.generator_version = "ooxml-foundation-4".into();
    assert!(unsupported.validate().is_err());
    assert!(report_docx::render(&unsupported).is_err());
}
