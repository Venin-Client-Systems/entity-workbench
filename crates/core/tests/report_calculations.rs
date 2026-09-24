use scraper::{Html, Selector};
use std::collections::BTreeSet;
use tempfile::TempDir;
use workbench_core::{domain::ReviewState, store::Workspace};

fn select(css: &str) -> Selector {
    Selector::parse(css).unwrap()
}

fn linked_ids(html: &Html, css: &str) -> BTreeSet<String> {
    html.select(&select(css))
        .map(|a| {
            a.value()
                .attr("href")
                .unwrap()
                .strip_prefix('#')
                .unwrap()
                .to_owned()
        })
        .collect()
}

#[test]
fn saved_calculations_link_exact_currency_partitions_and_versioned_source_rows() {
    let temp = TempDir::new_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    let original = b"account,date,description,amount,currency\n001,2025-03-01,Opening,100.00,AUD\n001,2025-03-02,<em onmouseover=unsafe()>Repeat</em>,-12.30000001,AUD\n001,2025-03-02,<em onmouseover=unsafe()>Repeat</em>,-12.30000001,AUD\n001,2025-03-03,Transfer out,-50.00,AUD\n002,2025-03-03,Transfer in,50.00,AUD\n001,2025-03-04,Foreign purchase,-7.10,USD\n001,2025-03-05,Unreviewed,1000.00,AUD\n001,2025-03-06,Rejected,2000.00,AUD\n001,2025-03-07,Deferred,3000.00,AUD\n";
    let evidence = workspace
        .import("synthetic-calculations.csv", original)
        .unwrap();
    let rows = workspace.view().unwrap().transactions;
    assert_eq!(rows.len(), 9);
    for row in &rows[..6] {
        workspace
            .review_transaction(
                &row.id,
                ReviewState::Accepted,
                "Synthetic review",
                workspace.revision().unwrap(),
            )
            .unwrap();
    }
    for (index, state) in [(7, ReviewState::Rejected), (8, ReviewState::Deferred)] {
        workspace
            .review_transaction(
                &rows[index].id,
                state,
                "Synthetic review",
                workspace.revision().unwrap(),
            )
            .unwrap();
    }
    workspace
        .match_transfer(
            &rows[3].id,
            &rows[4].id,
            "Synthetic counterpart review",
            workspace.revision().unwrap(),
        )
        .unwrap();
    let revision = workspace.revision().unwrap();
    workspace.save_report().unwrap();
    let snapshot = workspace.view().unwrap().reports[0].clone();
    assert_eq!(snapshot.workspace_revision, revision);
    let html = Html::parse_document(&snapshot.html);
    let aud = html
        .select(&select("[data-calculation-currency='AUD']"))
        .next()
        .unwrap();
    let text = aud.text().collect::<String>();
    assert!(
        text.contains("Credits 100.00 − debits 24.60000002 = net 75.39999998."),
        "{text}"
    );
    assert_eq!(
        linked_ids(
            &html,
            "[data-calculation-currency='AUD'] details:first-of-type a"
        ),
        rows[..3].iter().map(|t| t.id.clone()).collect()
    );
    assert_eq!(
        linked_ids(
            &html,
            "[data-calculation-currency='AUD'] details:last-of-type a"
        ),
        rows[3..5].iter().map(|t| t.id.clone()).collect()
    );
    assert_eq!(
        linked_ids(&html, "[data-calculation-currency='USD'] a"),
        BTreeSet::from([rows[5].id.clone()])
    );
    let usd = html
        .select(&select("[data-calculation-currency='USD']"))
        .next()
        .unwrap()
        .text()
        .collect::<String>();
    assert!(
        usd.contains("Credits 0 − debits 7.10 = net -7.10."),
        "{usd}"
    );
    let coverage = html
        .select(&select("#transaction-review-coverage"))
        .next()
        .unwrap();
    let summaries = coverage
        .select(&select("summary"))
        .map(|e| e.text().collect::<String>())
        .collect::<Vec<_>>();
    assert_eq!(
        summaries,
        [
            "Accepted records, including matched transfers: 6",
            "Pending records: 1",
            "Rejected records: 1",
            "Deferred records: 1"
        ]
    );
    assert_eq!(
        linked_ids(&html, "#transaction-review-coverage a"),
        rows.iter().map(|t| t.id.clone()).collect()
    );
    assert_eq!(html.select(&select("em, script, [onmouseover]")).count(), 0);
    // Independent HTML parsing checks every fragment resolves exactly once.
    let ids = html
        .select(&select("[id]"))
        .map(|e| e.value().attr("id").unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        ids.len(),
        ids.iter().copied().collect::<BTreeSet<_>>().len()
    );
    for link in html.select(&select("a")) {
        let target = link
            .value()
            .attr("href")
            .unwrap()
            .strip_prefix('#')
            .unwrap();
        assert_eq!(
            ids.iter().filter(|id| **id == target).count(),
            1,
            "{target}"
        );
    }
    for row in workspace.view().unwrap().transactions {
        let tr = html
            .select(&select("tr[id]"))
            .find(|tr| tr.value().attr("id") == Some(row.id.as_str()))
            .unwrap();
        let cells = tr
            .select(&select("td"))
            .map(|td| td.text().collect::<String>())
            .collect::<Vec<_>>();
        assert_eq!(cells[5], row.version.to_string());
        let anchor_json = tr
            .select(&select("td:last-child details code"))
            .next()
            .unwrap()
            .text()
            .collect::<String>();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&anchor_json).unwrap(),
            serde_json::to_value(&row.anchor).unwrap()
        );
        assert!(tr
            .select(&select("td:last-child a"))
            .next()
            .unwrap()
            .text()
            .collect::<String>()
            .starts_with("CSV · row "));
        assert_eq!(
            tr.select(&select("a")).next().unwrap().value().attr("href"),
            Some(format!("#{evidence}").as_str())
        );
    }
    workspace
        .correct_transaction(
            &rows[1].id,
            "-22.30",
            "Synthetic correction",
            workspace.revision().unwrap(),
        )
        .unwrap();
    workspace.save_report().unwrap();
    let current = workspace.view().unwrap();
    assert_eq!(current.reports[0].html, snapshot.html);
    assert_eq!(current.reports[0].sha256, snapshot.sha256);
    let corrected = Html::parse_document(&current.reports[1].html);
    assert_eq!(
        linked_ids(
            &corrected,
            "[data-calculation-currency='AUD'] details:first-of-type a"
        ),
        BTreeSet::from([rows[0].id.clone(), rows[2].id.clone()])
    );
    assert_eq!(
        std::fs::read(temp.path().join("case/originals").join(evidence)).unwrap(),
        original
    );
}

#[test]
fn pending_only_and_empty_workspaces_do_not_invent_reviewed_calculations() {
    let temp = TempDir::new_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    for source in [
        None,
        Some(
            b"account,date,description,amount,currency\n001,2025-03-01,Pending,2.00,AUD\n"
                .as_slice(),
        ),
    ] {
        if let Some(source) = source {
            workspace.import("pending.csv", source).unwrap();
        }
        workspace.save_report().unwrap();
        let view = workspace.view().unwrap();
        let snapshot = view.reports.last().unwrap();
        let html = Html::parse_document(&snapshot.html);
        assert_eq!(
            html.select(&select("[data-calculation-currency]")).count(),
            0
        );
        assert!(snapshot
            .html
            .contains("No accepted transactions are available for calculation."));
        let counts = html
            .select(&select("#transaction-review-coverage summary"))
            .map(|e| e.text().collect::<String>())
            .collect::<Vec<_>>();
        assert_eq!(
            counts[1],
            format!("Pending records: {}", usize::from(source.is_some()))
        );
    }
}
