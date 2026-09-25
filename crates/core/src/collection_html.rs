//! Bounded admission around the pinned HTML parser, followed by exact static extraction.
//! Counters bound the operations named here, not internal parser instructions or wall time.
use crate::{collection::PAGE_BYTES, policy};
use html5ever::{
    buffer_queue::BufferQueue,
    tendril::StrTendril,
    tokenizer::{Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts},
    tree_builder::{TreeBuilder, TreeSink},
    TokenizerResult,
};
use scraper::{Html, HtmlTreeSink, Selector};
use std::cell::Cell;
use url::Url;

type Handle = <HtmlTreeSink as TreeSink>::Handle;

#[derive(Clone, Copy)]
struct Limits {
    nodes: usize,
    depth: usize,
    tokens: usize,
    attributes: usize,
    pending_bytes: usize,
    admission_units: usize,
    text_bytes: usize,
    link_bytes: usize,
}
const LIMITS: Limits = Limits {
    nodes: 8192,
    depth: 128,
    tokens: 65_536,
    attributes: 128,
    pending_bytes: 16 * 1024,
    admission_units: 32 * 1024 * 1024,
    text_bytes: 512 * 1024,
    link_bytes: 512 * 1024,
};
const CHUNK_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HtmlLimit {
    Input,
    Tokens,
    Attributes,
    PendingToken,
    AdmissionUnits,
    Nodes,
    Depth,
    Text,
    Links,
}
impl std::fmt::Display for HtmlLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "HTML interpretation limit ({self:?}); no partial text or links"
        )
    }
}
#[derive(Default)]
struct Counters {
    tokens: Cell<usize>,
    completed: Cell<usize>,
    admission_units: Cell<usize>,
    maximum_nodes: Cell<usize>,
    refusal: Cell<Option<HtmlLimit>>,
}
struct BoundedBuilder {
    builder: TreeBuilder<Handle, HtmlTreeSink>,
    limits: Limits,
    counters: Counters,
}
impl BoundedBuilder {
    fn refuse(&self, reason: HtmlLimit) -> TokenSinkResult<Handle> {
        // Continue tokenization of at most the current 512-byte feed. No later
        // token is forwarded to the tree builder. In particular EOF cannot
        // synthesize or repair a partial tree after a refusal.
        self.counters.refusal.set(Some(reason));
        TokenSinkResult::Continue
    }
    fn nodes(&self) -> usize {
        self.builder.sink.0.borrow().tree.nodes().len()
    }
}
impl TokenSink for BoundedBuilder {
    type Handle = Handle;
    fn process_token(&self, token: Token, line: u64) -> TokenSinkResult<Handle> {
        if self.counters.refusal.get().is_some() {
            return TokenSinkResult::Continue;
        }
        let tokens = self.counters.tokens.get() + 1;
        self.counters.tokens.set(tokens);
        if tokens > self.limits.tokens {
            return self.refuse(HtmlLimit::Tokens);
        }
        let attributes = match &token {
            Token::TagToken(tag) => tag.attrs.len(),
            _ => 0,
        };
        if attributes > self.limits.attributes {
            return self.refuse(HtmlLimit::Attributes);
        }
        // A conservative admission counter for calls into the existing builder.
        // This is not a count of its internal steps; one call is not preemptible.
        let units = self.counters.admission_units.get() + 1 + self.nodes() + attributes;
        if units > self.limits.admission_units {
            return self.refuse(HtmlLimit::AdmissionUnits);
        }
        self.counters.admission_units.set(units);
        // Parse errors may occur while a very long tag/comment is still being
        // accumulated. They must not reset the pending-token byte window.
        if !matches!(token, Token::ParseError(_)) {
            self.counters
                .completed
                .set(self.counters.completed.get() + 1);
        }
        let result = self.builder.process_token(token, line);
        let nodes = self.nodes();
        self.counters
            .maximum_nodes
            .set(self.counters.maximum_nodes.get().max(nodes));
        if nodes > self.limits.nodes {
            return self.refuse(HtmlLimit::Nodes);
        }
        result
    }
    fn end(&self) {
        if self.counters.refusal.get().is_none() {
            self.builder.end();
        }
    }
    fn adjusted_current_node_present_but_not_in_html_namespace(&self) -> bool {
        self.builder
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}

fn parse(raw: &str, limits: Limits) -> Result<Html, HtmlLimit> {
    if raw.len() > PAGE_BYTES {
        return Err(HtmlLimit::Input);
    }
    let tokenizer = Tokenizer::new(
        BoundedBuilder {
            builder: TreeBuilder::new(HtmlTreeSink::new(Html::new_document()), Default::default()),
            limits,
            counters: Counters::default(),
        },
        TokenizerOpts {
            discard_bom: false,
            ..Default::default()
        },
    );
    let input = BufferQueue::default();
    let mut start = 0;
    let mut pending_bytes = 0;
    let mut legacy_feed_start = true;
    while start < raw.len() {
        let mut end = (start + CHUNK_BYTES).min(raw.len());
        while !raw.is_char_boundary(end) {
            end -= 1;
        }
        // The check precedes feeding. A single unfinished token therefore sees
        // at most pending_bytes plus one bounded chunk after the last completion.
        if pending_bytes + end - start > limits.pending_bytes {
            return Err(HtmlLimit::PendingToken);
        }
        let completed = tokenizer.sink.counters.completed.get();
        input.push_back(StrTendril::from_slice(&raw[start..end]));
        loop {
            // Pinned html5ever strips a BOM at EVERY feed call, not only at
            // document start. Preserve the old one-buffer driver's initial and
            // script-resumption checks, never our newly introduced chunk edges.
            if legacy_feed_start && !input.is_empty() {
                if input.peek() == Some('\u{feff}') {
                    input.next();
                }
                legacy_feed_start = false;
            }
            let result = tokenizer.feed(&input);
            if let Some(reason) = tokenizer.sink.counters.refusal.get() {
                return Err(reason);
            }
            if matches!(result, TokenizerResult::Done) {
                break;
            }
            // Match the pinned scraper driver: scripts are never executed.
            legacy_feed_start = true;
        }
        pending_bytes = if tokenizer.sink.counters.completed.get() == completed {
            pending_bytes + end - start
        } else {
            // Conservatively include the entire last chunk: its final bytes
            // may already belong to the next unfinished token.
            end - start
        };
        start = end;
    }
    tokenizer.end();
    if let Some(reason) = tokenizer.sink.counters.refusal.get() {
        return Err(reason);
    }
    Ok(tokenizer.sink.builder.sink.finish())
}

fn hidden(node: &scraper::Node) -> bool {
    node.as_element().is_some_and(|element| {
        ["script", "style", "svg", "noscript", "template"].contains(&element.name())
    })
}
fn extract_tree(html: &Html, base: &Url, limits: Limits) -> Result<(String, Vec<Url>), HtmlLimit> {
    let mut text = String::new();
    // A parent/first-child/next-sibling walk visits each connected node at most
    // twice, without recursion, ancestor scans or a stack proportional to width.
    let root = html.root_element();
    let mut current = *root;
    let mut entering = true;
    let mut depth = 0;
    let mut hidden_depth = 0;
    loop {
        if entering {
            depth += 1;
            if depth > limits.depth {
                return Err(HtmlLimit::Depth);
            }
            if hidden(current.value()) {
                hidden_depth += 1;
            }
            if hidden_depth == 0 {
                if let Some(value) = current.value().as_text() {
                    for word in value.text.split_whitespace() {
                        let separator = usize::from(!text.is_empty());
                        if text.len() + separator + word.len() > limits.text_bytes {
                            return Err(HtmlLimit::Text);
                        }
                        if separator != 0 {
                            text.push(' ');
                        }
                        text.push_str(word);
                    }
                }
            }
            if let Some(child) = current.first_child() {
                current = child;
                continue;
            }
        }
        if hidden(current.value()) {
            hidden_depth -= 1;
        }
        depth -= 1;
        if current.id() == root.id() {
            break;
        }
        if let Some(sibling) = current.next_sibling() {
            current = sibling;
            entering = true;
        } else {
            current = current.parent().expect("connected descendant");
            entering = false;
        }
    }
    // Preserve scraper's allocation-order selector semantics, including hidden
    // anchors, duplicates, and taking 1000 candidates BEFORE URL validation.
    let selector = Selector::parse("a[href]").expect("constant selector");
    let mut links = Vec::new();
    let mut bytes = 0;
    for anchor in html.select(&selector).take(1000) {
        if let Some(link) = anchor
            .value()
            .attr("href")
            .and_then(|href| base.join(href).ok())
            .and_then(|url| policy::validate_https_url(url.as_str()).ok())
        {
            bytes += link.as_str().len();
            if bytes > limits.link_bytes {
                return Err(HtmlLimit::Links);
            }
            links.push(link);
        }
    }
    Ok((text, links))
}

pub(crate) fn extract(raw: &str, base: &Url) -> Result<(String, Vec<Url>), HtmlLimit> {
    extract_tree(&parse(raw, LIMITS)?, base, LIMITS)
}

#[cfg(test)]
mod tests;
