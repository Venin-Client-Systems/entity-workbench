//! Closed synthetic font cases, shared by matched native controls and workers.
use super::{bounded, Result};
use serde_json::Value;

pub(crate) const PARSER: &str = super::PDF_FONT_PARSER;
#[derive(Clone, Copy)]
pub(crate) enum FontFixture {
    Corpus,
    Embedded,
}
impl FontFixture {
    pub(crate) fn bytes(self) -> &'static [u8] {
        match self {
            Self::Corpus => include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../fixtures/parser-fonts/corpus.pdf"
            )),
            Self::Embedded => include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../fixtures/parser-fonts/embedded.pdf"
            )),
        }
    }
    pub(crate) fn validate(self, reply: &Value) -> Result<()> {
        let text = reply["text"].as_str().unwrap_or("");
        let limitations = reply["limitations"].as_array();
        let has = |name: &str| limitations.is_some_and(|items| items.iter().any(|v| v == name));
        bounded(
            reply["parser"] == PARSER && reply["status"] == "partial" && reply["error"].is_null(),
            "font policy identity/status mismatch",
        )?;
        let pages = if matches!(self, Self::Corpus) {
            "21"
        } else {
            "1"
        };
        bounded(
            reply["metadata"]["pdf:pages"] == serde_json::json!([pages]),
            "font fixture page count mismatch",
        )?;
        match self {
            Self::Embedded => bounded(
                text.trim() == "Embedded é Ω Ж"
                    && !has("font_substituted")
                    && !has("font_coverage_unverified"),
                "embedded font text or fallback mismatch",
            ),
            Self::Corpus => {
                for face in [
                    "COURIER",
                    "COURIER_BOLD",
                    "COURIER_OBLIQUE",
                    "COURIER_BOLD_OBLIQUE",
                    "HELVETICA",
                    "HELVETICA_BOLD",
                    "HELVETICA_OBLIQUE",
                    "HELVETICA_BOLD_OBLIQUE",
                    "TIMES_ROMAN",
                    "TIMES_BOLD",
                    "TIMES_ITALIC",
                    "TIMES_BOLD_ITALIC",
                ] {
                    bounded(
                        section(text, &format!("STD_{face}"))?
                            == format!("Standard14 café 123 € {face}"),
                        "standard font text mismatch",
                    )?;
                }
                for (label, value) in [
                    ("STD_SYMBOL", "ΑΒ"),
                    ("STD_ZAPF_DINGBATS", "✁✂"),
                    ("EMBEDDED", "Embedded é Ω Ж"),
                    ("UNKNOWN_WIDTHS", "Unknown explicit widths"),
                    ("UNKNOWN_NO_WIDTHS", "Unknown missing widths"),
                    ("DIFFERENCES", "éfiΓ"),
                    ("SIMPLE_UNICODE", "一😀ا"),
                    ("CID_UNICODE", "一😀ا"),
                    ("CID_UNMAPPED", ""),
                ] {
                    bounded(
                        section(text, label)? == value,
                        "font encoding/coverage case mismatch",
                    )?;
                }
                bounded(
                    has("font_substituted") && has("font_coverage_unverified"),
                    "font fallback limitations absent",
                )
            }
        }
    }
}

fn section(text: &str, label: &str) -> Result<String> {
    let begin = format!("BEGIN_{label}");
    let end = format!("END_{label}");
    let lines: Vec<_> = text.lines().collect();
    bounded(
        lines.iter().filter(|line| **line == begin).count() == 1
            && lines.iter().filter(|line| **line == end).count() == 1,
        "font case markers absent or repeated",
    )?;
    let start = lines.iter().position(|line| *line == begin).unwrap();
    let stop = lines.iter().position(|line| *line == end).unwrap();
    bounded(stop > start, "font case marker order mismatch")?;
    Ok(lines[start + 1..stop].join("\n").trim().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separate_unicode_cases_cannot_mask_absent_or_unmapped_pages() {
        let text = "BEGIN_SIMPLE_UNICODE\n一😀ا\nEND_SIMPLE_UNICODE\nBEGIN_CID_UNMAPPED\nEND_CID_UNMAPPED\n";
        assert_eq!(section(text, "SIMPLE_UNICODE").unwrap(), "一😀ا");
        assert_eq!(section(text, "CID_UNMAPPED").unwrap(), "");
        assert!(section(text, "CID_UNICODE").is_err());
        assert!(section(&format!("{text}{text}"), "SIMPLE_UNICODE").is_err());
        assert!(section("END_CASE\nBEGIN_CASE", "CASE").is_err());
    }
    #[test]
    fn embedded_case_rejects_old_policy_and_false_fallback() {
        let mut result = serde_json::json!({"parser":PARSER,"status":"partial","error":null,"text":"Embedded é Ω Ж","limitations":[],"metadata":{"pdf:pages":["1"]}});
        assert!(FontFixture::Embedded.validate(&result).is_ok());
        result["parser"] = "pdfbox-3.0.8".into();
        assert!(FontFixture::Embedded.validate(&result).is_err());
        result["parser"] = PARSER.into();
        result["limitations"] = serde_json::json!(["font_substituted"]);
        assert!(FontFixture::Embedded.validate(&result).is_err());
        assert!(FontFixture::Corpus.validate(&result).is_err());
        assert!(FontFixture::Corpus.bytes().len() < 16 * 1024 * 1024);
        assert!(FontFixture::Embedded.bytes().len() < 16 * 1024 * 1024);
    }
}
