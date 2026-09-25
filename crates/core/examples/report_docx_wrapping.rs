//! Fixed synthetic generator comparison; no canonical writes or arbitrary document input.
use std::{fs, io::Write, path::PathBuf};
use workbench_core::{
    report_document::{self, ReportDocument},
    report_docx, Result,
};

fn main() -> Result<()> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("new output directory required"),
    );
    fs::create_dir(&output)?;
    let mut document =
        ReportDocument::from_json(include_bytes!("../tests/fixtures/report-generator1.json"))?;
    document.generator_version = report_document::GENERATOR_VERSION.into();
    write(&output, "ordinary", &document)?;
    document.content.findings[0].limitations = format!(
        "Synthetic long literal: {}. Ordinary prose still needs sensible wrapping.",
        "W".repeat(160)
    );
    document.content.transactions[1].description = format!(
        "Literal {} followed by ordinary purchase description.",
        "W".repeat(160)
    );
    write(&output, "long-literals", &document)
}
fn write(output: &std::path::Path, name: &str, document: &ReportDocument) -> Result<()> {
    for (extension, bytes) in [
        ("json", document.to_json()?),
        ("docx", report_docx::render(document)?),
    ] {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(format!("{name}.{extension}")))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    Ok(())
}
