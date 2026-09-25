//! Post-reap parsing only. Invalid diagnostics never replace the worker outcome.
use super::*;

const LIMIT: u64 = 512;
const CLOCK_LIMIT: u64 = 120_000;
const ATTEMPTS: [&str; 16] = [
    "numpy",
    "numpy._core._multiarray_umath",
    "catalogue",
    "confection",
    "thinc",
    "thinc.compat",
    "thinc.backends.numpy_ops",
    "blis",
    "blis.cy",
    "srsly",
    "pydantic_core",
    "pydantic_core._pydantic_core",
    "spacy.pipeline",
    "spacy.language",
    "spacy.cli",
    "weasel",
];

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TimedCheckpoint {
    module: String,
    boundary: String,
    elapsed_ms: u64,
    process_cpu_ms: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Attempt {
    module: String,
    ordinal: usize,
    elapsed_ms: u64,
    process_cpu_ms: u64,
}

#[derive(Debug, Default, Serialize)]
pub(super) struct Observation {
    valid: bool,
    checkpoints: Vec<TimedCheckpoint>,
    attempts: Vec<Attempt>,
}

fn increasing(previous: (u64, u64), current: (u64, u64)) -> bool {
    current.0 >= previous.0
        && current.1 >= previous.1
        && current.0 <= CLOCK_LIMIT
        && current.1 <= CLOCK_LIMIT
}

impl Observation {
    pub(super) fn last(&self) -> Option<ImportCheckpoint> {
        self.checkpoints.last().map(|last| ImportCheckpoint {
            module: last.module.clone(),
            boundary: last.boundary.clone(),
        })
    }

    pub(super) fn complete(&self) -> bool {
        self.valid && self.checkpoints.len() == 12
    }
}

fn collect_checkpoints(job: &Path, result: &mut Observation) -> Result<()> {
    let mut previous = (0, 0);
    let mut missing = false;
    for index in 0..12 {
        let path = job.join(format!("scratch/import-{index}.json"));
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing = true;
                continue;
            }
            other => {
                other?;
            }
        }
        require(!missing, "Import diagnostic sequence has a gap")?;
        let record: TimedCheckpoint = serde_json::from_slice(&read_result(&path, LIMIT)?)?;
        let current = (record.elapsed_ms, record.process_cpu_ms);
        require(
            record.module == IMPORTS[index / 2]
                && record.boundary == ["before", "after"][index % 2]
                && increasing(previous, current),
            "Invalid import diagnostic checkpoint",
        )?;
        previous = current;
        result.checkpoints.push(record);
    }
    Ok(())
}

fn collect_attempts(job: &Path, result: &mut Observation) -> Result<()> {
    let mut previous = (0, 0);
    let mut seen = BTreeSet::new();
    let mut missing = false;
    for index in 0..16 {
        let path = job.join(format!("scratch/import-attempt-{index}.json"));
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing = true;
                continue;
            }
            other => {
                other?;
            }
        }
        require(!missing, "Import attempt sequence has a gap")?;
        let record: Attempt = serde_json::from_slice(&read_result(&path, LIMIT)?)?;
        let current = (record.elapsed_ms, record.process_cpu_ms);
        require(
            record.ordinal == index
                && ATTEMPTS.contains(&record.module.as_str())
                && seen.insert(record.module.clone())
                && increasing(previous, current),
            "Invalid import attempt diagnostic",
        )?;
        previous = current;
        result.attempts.push(record);
    }
    Ok(())
}

fn reject_extra_records(job: &Path) -> Result<()> {
    let allowed: BTreeSet<_> = (0..12)
        .map(|i| format!("import-{i}.json"))
        .chain((0..16).map(|i| format!("import-attempt-{i}.json")))
        .collect();
    for (index, entry) in fs::read_dir(job.join("scratch"))?.enumerate() {
        require(
            index < 512,
            "Import diagnostic directory exceeds entry limit",
        )?;
        let name = entry?.file_name();
        if let Some(name) = name.to_str().filter(|name| name.starts_with("import-")) {
            require(
                allowed.contains(name),
                "Unexpected import diagnostic record",
            )?;
        }
    }
    Ok(())
}

pub(super) fn collect(job: &Path) -> Observation {
    if reject_linked_ancestors(&job.join("scratch")).is_err() {
        return Observation::default();
    }
    let mut result = Observation {
        valid: true,
        ..Observation::default()
    };
    // Retain each validated contiguous prefix even if the next file is invalid.
    // Run each reader independently so one bad stream cannot erase the other.
    let checkpoints = collect_checkpoints(job, &mut result);
    let attempts = collect_attempts(job, &mut result);
    let count = reject_extra_records(job);
    result.valid = checkpoints.is_ok() && attempts.is_ok() && count.is_ok();
    result
}

#[cfg(test)]
mod tests;
