//! Strict parser for the fixed Tesseract 5 TSV recipe, with companion text completeness checks.
use super::*;
use std::collections::BTreeSet;
const HEADER: &str =
    "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext";

fn integer(value: &str) -> Result<u32> {
    let number = value
        .parse::<u32>()
        .map_err(|_| Error::Validation("Invalid OCR TSV integer".into()))?;
    require(value == number.to_string(), "Noncanonical OCR TSV integer")?;
    Ok(number)
}
fn contained(child: &RasterBox, parent: &RasterBox) -> bool {
    child.width > 0
        && child.height > 0
        && child.left >= parent.left
        && child.top >= parent.top
        && child.left.checked_add(child.width).is_some_and(|right| {
            parent
                .left
                .checked_add(parent.width)
                .is_some_and(|edge| right <= edge)
        })
        && child.top.checked_add(child.height).is_some_and(|bottom| {
            parent
                .top
                .checked_add(parent.height)
                .is_some_and(|edge| bottom <= edge)
        })
}

pub(super) fn parse(bytes: &[u8], text: &str, width: u32, height: u32) -> Result<Vec<OcrRegion>> {
    require(
        !bytes.is_empty() && bytes.len() < MAX_TSV_BYTES as usize,
        "OCR TSV is empty, truncated or oversized",
    )?;
    ocr::validate_text(text)?;
    require(
        text.trim().is_empty() || text.ends_with('\n'),
        "OCR companion text final line is truncated",
    )?;
    require(bytes.ends_with(b"\n"), "OCR TSV final row is truncated")?;
    let tsv =
        std::str::from_utf8(bytes).map_err(|_| Error::Validation("OCR TSV is not UTF-8".into()))?;
    let mut lines = tsv[..tsv.len() - 1].split('\n');
    require(lines.next() == Some(HEADER), "Unexpected OCR TSV header")?;
    let mut regions: Vec<OcrRegion> = Vec::new();
    let mut parents: [Option<usize>; 4] = [None; 4];
    let mut numbering = [0u32; 4];
    let mut previous_level = 0;
    let mut words = 0;
    let mut word_bytes = 0;
    let mut unique_words = BTreeSet::new();
    let mut expected_words = text.split_whitespace();
    for line in lines {
        require(
            regions.len() < MAX_REGIONS && line.len() <= MAX_WORD_BYTES + 160,
            "OCR TSV row count or length exceeds policy",
        )?;
        let fields: Vec<_> = line.split('\t').collect();
        require(fields.len() == 12, "OCR TSV requires exactly twelve fields")?;
        let level = integer(fields[0])?;
        require((1..=5).contains(&level), "Invalid OCR TSV level")?;
        let page_number = integer(fields[1])?;
        require(page_number == 1, "OCR TSV contains another raster page")?;
        let ids = [
            integer(fields[2])?,
            integer(fields[3])?,
            integer(fields[4])?,
            integer(fields[5])?,
        ];
        let bounds = RasterBox {
            left: integer(fields[6])?,
            top: integer(fields[7])?,
            width: integer(fields[8])?,
            height: integer(fields[9])?,
        };
        let (kind, confidence) = if level == 1 {
            require(
                regions.is_empty()
                    && ids == [0; 4]
                    && bounds
                        == RasterBox {
                            left: 0,
                            top: 0,
                            width,
                            height,
                        },
                "OCR TSV page does not match the assigned raster",
            )?;
            (RegionLevel::Page, None)
        } else {
            require(
                previous_level > 0 && (level == previous_level + 1 || previous_level == 5),
                "OCR TSV hierarchy is missing, empty or out of order",
            )?;
            let index = (level - 2) as usize;
            numbering[index] += 1;
            numbering[index + 1..].fill(0);
            require(
                ids == numbering,
                "OCR TSV hierarchy IDs are duplicate, skipped or out of order",
            )?;
            let parent = parents[index]
                .and_then(|index| regions.get(index))
                .ok_or_else(|| Error::Validation("OCR TSV parent is absent".into()))?;
            require(
                contained(&bounds, &parent.bounds),
                "OCR TSV box escapes its parent or is empty",
            )?;
            match level {
                2 => (RegionLevel::Block, None),
                3 => (RegionLevel::Paragraph, None),
                4 => (RegionLevel::Line, None),
                5 => {
                    let (whole, decimal) = fields[10]
                        .split_once('.')
                        .ok_or_else(|| Error::Validation("Malformed OCR confidence".into()))?;
                    integer(whole)?;
                    require(
                        decimal.len() == 6 && decimal.bytes().all(|c| c.is_ascii_digit()),
                        "Malformed OCR confidence precision",
                    )?;
                    let confidence: f64 = fields[10]
                        .parse()
                        .map_err(|_| Error::Validation("Invalid OCR word confidence".into()))?;
                    require(
                        confidence.is_finite() && (0.0..=100.0).contains(&confidence),
                        "OCR word confidence is out of range",
                    )?;
                    let word = fields[11];
                    require(
                        !word.is_empty()
                            && word.len() <= MAX_WORD_BYTES
                            && !word.chars().any(|c| c.is_control() || c.is_whitespace()),
                        "Invalid or oversized OCR word",
                    )?;
                    words += 1;
                    word_bytes += word.len();
                    require(
                        words <= MAX_WORDS && word_bytes <= ocr::MAX_TEXT_BYTES as usize,
                        "OCR words exceed policy",
                    )?;
                    require(
                        unique_words.insert((
                            bounds.left,
                            bounds.top,
                            bounds.width,
                            bounds.height,
                            word,
                        )),
                        "OCR TSV contains a duplicated word box",
                    )?;
                    require(
                        expected_words.next() == Some(word),
                        "OCR TSV words disagree with companion text",
                    )?;
                    (RegionLevel::Word, Some(confidence))
                }
                _ => unreachable!(),
            }
        };
        if level < 5 {
            require(
                fields[10] == "-1" && fields[11].is_empty(),
                "OCR structural row contains word confidence or text",
            )?;
            parents[(level - 1) as usize] = Some(regions.len());
            parents[level as usize..].fill(None);
        }
        regions.push(OcrRegion {
            level: kind,
            page_number,
            block_number: ids[0],
            paragraph_number: ids[1],
            line_number: ids[2],
            word_number: ids[3],
            bounds,
            engine_confidence: confidence,
            text: fields[11].into(),
        });
        previous_level = level;
    }
    require(
        previous_level == 1 || previous_level == 5,
        "OCR TSV ends in an incomplete hierarchy",
    )?;
    require(
        expected_words.next().is_none(),
        "OCR TSV is missing recognized words",
    )?;
    require(
        (words == 0) == text.trim().is_empty(),
        "OCR TSV empty status conflicts with text",
    )?;
    Ok(regions)
}
