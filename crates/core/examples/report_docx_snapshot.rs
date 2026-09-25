//! Fixed synthetic canonical publication and recovery proof; no analyst or arbitrary JSON input.
use std::{fs, io::Write, path::PathBuf};
use workbench_core::{domain::*, store::Workspace, Result};

fn main() -> Result<()> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("new output directory required"),
    );
    fs::create_dir(&output)?;
    let mut workspace = Workspace::open(output.join("workspace"))?;
    let source = workspace.import("synthetic-payments.csv", b"account,date,description,amount,currency\n001,2025-03-01,Opening credit,100.00,AUD\n001,2025-03-02,Purchase,-12.30000001,AUD\n001,2025-03-03,Unreviewed,900.00,AUD\n")?;
    for row in workspace.view()?.transactions.into_iter().take(2) {
        workspace.review_transaction(
            &row.id,
            ReviewState::Accepted,
            "Synthetic review",
            workspace.revision()?,
        )?;
    }
    let finding = workspace.add_finding(FindingInput {
        title: "Reviewed payments".into(),
        assessment: "Reviewed credit and purchase records have an exact net movement of AUD 87.69999999. The unreviewed row is excluded from that total.".into(),
        supporting_ids: vec![source],
        contradicting_ids: vec![],
        limitations: "Synthetic records demonstrate immutable publication and recovery; they establish no real activity.".into(),
        hypothesis_ids: vec![],
    }, workspace.revision()?)?;
    workspace.review_finding(
        &finding,
        "Reviewed retained original",
        workspace.revision()?,
    )?;
    workspace.save_report()?;
    let record = workspace.save_docx_snapshot(
        "11111111-2222-4333-8444-555555555555",
        workspace.revision()?,
    )?;
    let inspected = workspace.inspect_docx_snapshot(
        &record.id,
        &record.document.sha256,
        &record.docx.sha256,
    )?;
    let bytes =
        workspace.read_docx_snapshot(&record.id, &record.document.sha256, &record.docx.sha256)?;
    let backup = workspace.backup()?;
    let restored = Workspace::restore(&backup, &output.join("restored"))?;
    assert_eq!(
        restored.read_docx_snapshot(&record.id, &record.document.sha256, &record.docx.sha256)?,
        bytes
    );
    for (name, bytes) in [
        ("assessment.docx", bytes),
        ("assessment.json", inspected.document.to_json()?),
        ("snapshot.json", serde_json::to_vec_pretty(&record)?),
    ] {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(name))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    Ok(())
}
