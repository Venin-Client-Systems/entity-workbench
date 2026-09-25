//! Fixed synthetic inputs; no caller-selected documents or expected output.
use super::{check, Failure, ProbeResult};
use serde_json::Value;
pub(super) const PDF_FONT_PARSER: &str = "pdfbox-3.0.8-local-fonts-v1";
type Result<T> = ProbeResult<T>;
fn bounded(ok: bool, _: &'static str) -> Result<()> {
    check(ok, Failure::Fixture)
}
// Reuse the reviewed, closed test oracle and compiled synthetic PDFs. This is
// example/test code only, with no worker execution or production dependency.
#[path = "../../../windows-worker/src/java/font_fixtures.rs"]
#[allow(dead_code)] // The shared oracle's fixture-byte helper is used by its own tests.
mod font_fixtures;

#[derive(Clone, Copy)]
pub(super) struct Fixture {
    pub name: &'static str,
    pub bytes: &'static [u8],
    pub state: &'static str,
    pub status: &'static str,
    pub parser: &'static str,
    pub error: Option<&'static str>,
}
const TXT: &[u8] = include_bytes!("../../../../fixtures/parser/notice.txt");
pub(super) const FIXTURES: [Fixture; 10] = [
    Fixture {
        name: "notice.txt",
        bytes: TXT,
        state: "completed",
        status: "complete",
        parser: "utf8-v1",
        error: None,
    },
    Fixture {
        name: "unreviewed.source",
        bytes: b"Synthetic unreviewed UTF-8 source. None must remain None.\n",
        state: "completed",
        status: "complete",
        parser: "utf8-v1",
        error: None,
    },
    Fixture {
        name: "notice.pdf",
        bytes: include_bytes!("../../../../fixtures/parser/notice.pdf"),
        state: "partial",
        status: "partial",
        parser: PDF_FONT_PARSER,
        error: None,
    },
    Fixture {
        name: "notice.docx",
        bytes: include_bytes!("../../../../fixtures/parser/notice.docx"),
        state: "partial",
        status: "partial",
        parser: "tika-ooxml-3.3.2",
        error: None,
    },
    Fixture {
        name: "no-text.pdf",
        bytes: include_bytes!("../../../../fixtures/parser/no-text.pdf"),
        state: "partial",
        status: "partial",
        parser: PDF_FONT_PARSER,
        error: None,
    },
    Fixture {
        name: "malformed.pdf",
        bytes: b"%PDF-synthetic malformed input\n",
        state: "failed",
        status: "failed",
        parser: PDF_FONT_PARSER,
        error: Some("malformed_document"),
    },
    Fixture {
        name: "traversal.zip",
        bytes: include_bytes!("../../../../fixtures/parser/traversal.zip"),
        state: "failed",
        status: "failed",
        parser: "zip-preflight-v1",
        error: Some("archive_limits"),
    },
    Fixture {
        name: "unsupported.bin",
        bytes: b"\0\xff\x80synthetic unsupported",
        state: "blocked",
        status: "unsupported",
        parser: "unsupported-v1",
        error: None,
    },
    Fixture {
        name: "font-corpus.pdf",
        bytes: include_bytes!("../../../../fixtures/parser-fonts/corpus.pdf"),
        state: "partial",
        status: "partial",
        parser: PDF_FONT_PARSER,
        error: None,
    },
    Fixture {
        name: "embedded-font.pdf",
        bytes: include_bytes!("../../../../fixtures/parser-fonts/embedded.pdf"),
        state: "partial",
        status: "partial",
        parser: PDF_FONT_PARSER,
        error: None,
    },
];
impl Fixture {
    pub fn validate(&self, result: &Value) -> ProbeResult<()> {
        check(
            result["status"] == self.status
                && result["parser"] == self.parser
                && result["error"] == self.error.map(Value::from).unwrap_or(Value::Null),
            Failure::Fixture,
        )?;
        let text = result["text"].as_str().ok_or(Failure::Fixture)?;
        match self.name {
            "notice.txt" | "unreviewed.source" => {
                check(text.as_bytes() == self.bytes, Failure::Fixture)
            }
            "notice.pdf" | "notice.docx" => {
                check(
                    text.contains("Rowan Ellis") && text.contains("Fictional Harbour Cooperative"),
                    Failure::Fixture,
                )?;
                if self.name == "notice.pdf" {
                    check(
                        result["metadata"]["pdf:pages"] == serde_json::json!(["1"])
                            && has(result, "font_substituted")
                            && has(result, "font_coverage_unverified"),
                        Failure::Fixture,
                    )?;
                }
                Ok(())
            }
            "no-text.pdf" => font_fixtures::validate_no_text_pdf(result),
            "font-corpus.pdf" => font_fixtures::FontFixture::Corpus.validate(result),
            "embedded-font.pdf" => font_fixtures::FontFixture::Embedded.validate(result),
            _ => check(
                text.is_empty() && result["metadata"] == serde_json::json!({}),
                Failure::Fixture,
            ),
        }
    }
}
fn has(value: &Value, name: &str) -> bool {
    value["limitations"]
        .as_array()
        .is_some_and(|items| items.iter().any(|v| v == name))
}
