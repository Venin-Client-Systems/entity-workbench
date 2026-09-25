//! Developer-only fixed synthetic document. No arbitrary input or canonical writes.
#[path = "support/report_fixture.rs"]
mod report_fixture;
use std::{fs, io::Write, path::PathBuf};
use workbench_core::{report_document, report_docx, Result};
fn main() -> Result<()> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("output directory required"),
    );
    fs::create_dir(&output)?;
    let document = report_document::capture(
        &report_fixture::fixture(),
        "11111111-2222-4333-8444-555555555555",
        "2025-03-10T12:00:00Z",
    )?;
    for (name, bytes) in [
        ("assessment.json", document.to_json()?),
        ("assessment.docx", report_docx::render(&document)?),
    ] {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(name))?
            .write_all(&bytes)?;
    }
    Ok(())
}
