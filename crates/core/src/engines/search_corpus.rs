//! Bounded, opaque input to the existing Lucene recipe. This is not an acquisition policy.
use crate::{domain::Evidence, require, Result};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    io::{self, Write},
};

pub(crate) const MAX_EVIDENCE_ROWS: usize = 100_000;
pub(crate) const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_DOCUMENTS: usize = 10_000;

pub(crate) fn validate_query(query: &str) -> Result<()> {
    require(
        !query.trim().is_empty() && query.len() <= 1024,
        "Query must contain 1 to 1024 bytes",
    )
}

/// Not deserializable or cloneable: callers can obtain only a fully admitted corpus.
pub(crate) struct SearchCorpus {
    revision: u64,
    #[cfg(any(test, target_os = "macos"))]
    manifest: Vec<u8>,
    #[cfg(any(test, target_os = "macos"))]
    document_count: u64,
    #[cfg(any(test, target_os = "macos"))]
    known_evidence_ids: BTreeSet<String>,
}
impl SearchCorpus {
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn manifest(&self) -> &[u8] {
        &self.manifest
    }
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn document_count(&self) -> u64 {
        self.document_count
    }
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn knows(&self, id: &str) -> bool {
        self.known_evidence_ids.contains(id)
    }
}

struct LimitedJson {
    bytes: Vec<u8>,
}
impl Write for LimitedJson {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        self.bytes
            .len()
            .checked_add(input.len())
            .filter(|size| *size <= MAX_MANIFEST_BYTES)
            .ok_or_else(|| io::Error::other("Index manifest exceeds the development limit"))?;
        // Admission precedes growth, including every JSON escaping fragment.
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) struct CorpusBuilder {
    revision: u64,
    rows: usize,
    documents: usize,
    failed: bool,
    writer: LimitedJson,
    ids: BTreeSet<String>,
}
impl CorpusBuilder {
    pub(crate) fn new(revision: u64) -> Result<Self> {
        let mut writer = LimitedJson { bytes: Vec::new() };
        // Preserve serde_json::Value's original sorted object-key order exactly.
        writer.write_all(b"{\"documents\":[")?;
        Ok(Self {
            revision,
            rows: 0,
            documents: 0,
            failed: false,
            writer,
            ids: BTreeSet::new(),
        })
    }
    pub(crate) fn push(&mut self, evidence: &Evidence) -> Result<()> {
        require(!self.failed, "Search corpus admission already failed")?;
        self.failed = true;
        require(
            self.rows < MAX_EVIDENCE_ROWS,
            "Search evidence row limit exceeded",
        )?;
        require(
            evidence.id.len() <= 64,
            "Search evidence identifier exceeds digest size",
        )?;
        self.rows += 1;
        self.ids.insert(evidence.id.clone());
        if let Some(text) = &evidence.text {
            require(
                self.documents < MAX_DOCUMENTS,
                "Search indexed document limit exceeded",
            )?;
            if self.documents != 0 {
                self.writer.write_all(b",")?;
            }
            #[derive(Serialize)]
            struct Document<'a> {
                id: &'a str,
                name: &'a str,
                text: &'a str,
            }
            serde_json::to_writer(
                &mut self.writer,
                &Document {
                    id: &evidence.id,
                    name: &evidence.name,
                    text,
                },
            )?;
            self.documents += 1;
        }
        self.failed = false;
        Ok(())
    }
    pub(crate) fn finish(mut self) -> Result<SearchCorpus> {
        require(!self.failed, "Search corpus admission already failed")?;
        self.writer.write_all(b"],\"workspace_revision\":")?;
        serde_json::to_writer(&mut self.writer, &self.revision)?;
        self.writer.write_all(b"}")?;
        Ok(SearchCorpus {
            revision: self.revision,
            #[cfg(any(test, target_os = "macos"))]
            manifest: self.writer.bytes,
            #[cfg(any(test, target_os = "macos"))]
            document_count: self.documents as u64,
            #[cfg(any(test, target_os = "macos"))]
            known_evidence_ids: self.ids,
        })
    }
}

#[cfg(test)]
#[path = "search_corpus_tests.rs"]
mod tests;
