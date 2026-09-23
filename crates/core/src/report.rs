use crate::{analytics, domain::*, Result};
use std::fmt::Write;
pub fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn citation(view: &WorkspaceView, key: &str) -> String {
    if let Some(o) = view.observations.iter().find(|o| o.id == key) {
        let name = view
            .entities
            .iter()
            .find(|e| e.id == o.entity_id)
            .map(|e| e.name.as_str())
            .unwrap_or(&o.entity_id);
        format!("{name} · {}: {} · {:?}", o.field, o.value, o.review)
    } else if let Some(t) = view.transactions.iter().find(|t| t.id == key) {
        format!(
            "{} · {} · {} {} · {:?}",
            t.date, t.description, t.amount, t.currency, t.review
        )
    } else if let Some(e) = view.evidence.iter().find(|e| e.id == key) {
        format!("Whole source: {}", e.name)
    } else {
        format!("Unresolved citation: {key}")
    }
}
pub fn html(view: &WorkspaceView, report_id: &str) -> Result<String> {
    let mut out=format!("<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\"><title>Entity Workbench assessment</title><style>body{{font:16px/1.6 system-ui;max-width:1100px;margin:48px auto;padding:0 24px;color:#172d31}}h1,h2{{line-height:1.2}}table{{width:100%;border-collapse:collapse;font-size:13px}}th,td{{text-align:left;padding:10px;border-bottom:1px solid #ccc;overflow-wrap:anywhere}}code{{overflow-wrap:anywhere}}.notice{{background:#fff1d6;padding:16px}}@media print{{body{{margin:0}}tr{{break-inside:avoid}}}}</style><h1>Investigation assessment</h1><p>Snapshot {} · Workspace revision {}</p><p class=\"notice\">Development assessment. Pending evidence is not an accepted conclusion. Amounts are exact decimal values, grouped by currency. Transfers are excluded only after explicit matching. Source origin groups do not establish independence by themselves.</p>",escape(report_id),view.revision);
    out.push_str("<h2>Questions and alternatives</h2>");
    for h in &view.hypotheses {
        let _ = write!(
            out,
            "<h3 id=\"{}\">{}</h3><p>{}</p><p>Alternatives: {}</p><p>Collection gaps: {}</p>",
            escape(&h.id),
            escape(&h.question),
            escape(&h.proposition),
            escape(&h.alternatives.join("; ")),
            escape(&h.gaps.join("; "))
        );
    }
    out.push_str("<h2>Findings</h2>");
    for f in &view.findings {
        let _ = write!(
            out,
            "<h3>{}</h3><p>{}</p><p>Review required: {}</p><p>Limitations: {}</p><p>Supporting: ",
            escape(&f.title),
            escape(&f.assessment),
            f.needs_review,
            escape(&f.limitations)
        );
        for key in &f.supporting_ids {
            let _ = write!(
                out,
                "<a href=\"#{}\">{}</a> ",
                escape(key),
                escape(&citation(view, key))
            );
        }
        out.push_str("</p><p>Contradicting: ");
        for key in &f.contradicting_ids {
            let _ = write!(
                out,
                "<a href=\"#{}\">{}</a> ",
                escape(key),
                escape(&citation(view, key))
            );
        }
        out.push_str("</p><p>Linked questions: ");
        for key in &f.hypothesis_ids {
            let label = view
                .hypotheses
                .iter()
                .find(|h| &h.id == key)
                .map(|h| h.question.as_str())
                .unwrap_or("Unresolved question");
            let _ = write!(out, "<a href=\"#{}\">{}</a> ", escape(key), escape(label));
        }
        out.push_str("</p>");
    }
    let analysis = analytics::analyse(&view.transactions)?;
    out.push_str("<h2>Reviewed transaction calculations</h2>");
    for t in analysis.totals {
        let _=write!(out,"<p>{}: credits {} − debits {} = net {}. Included records: {}; explicitly matched transfer records excluded: {}.</p>",escape(&t.currency),escape(&t.credits),escape(&t.debits),escape(&t.net),t.transaction_ids.len(),t.excluded_transfer_ids.len());
    }
    let _=write!(out,"<p>{} transactions pending review. No currency conversion performed. Balance checks include all intervening source rows between available balances within each imported account and currency; opening balances are not inferred.</p>",analysis.pending);
    out.push_str("<h2>Transaction exhibit</h2><table><tr><th>Date</th><th>Account</th><th>Original description</th><th>Amount</th><th>Review</th><th>Source</th></tr>");
    for t in &view.transactions {
        let _=write!(out,"<tr id=\"{}\"><td>{}</td><td>{}</td><td>{}</td><td>{} {}</td><td>{:?}</td><td><a href=\"#{}\">{}</a></td></tr>",escape(&t.id),escape(&t.date),escape(&t.account),escape(&t.description),escape(&t.amount),escape(&t.currency),t.review,escape(t.anchor.evidence_id()),escape(&serde_json::to_string(&t.anchor)?));
    }
    out.push_str("</table><h2>Entity register</h2>");
    for e in &view.entities {
        let _ = write!(out, "<section id=\"{}\"><h3>{}</h3><p>Kind: {:?}<br>Reference Numbers: {}<br>Merged into: {}</p></section>", escape(&e.id), escape(&e.name), e.kind, escape(&e.identifiers.iter().map(|i| format!("{}:{}", i.namespace, i.value)).collect::<Vec<_>>().join("; ")), escape(e.merged_into.as_deref().unwrap_or("Separate record")));
    }
    out.push_str("<h2>Identity decisions</h2>");
    for d in &view.identity_decisions {
        let _ = write!(
            out,
            "<p>{} / {} · {:?} · {}<br>{}</p>",
            escape(&d.left_id),
            escape(&d.right_id),
            d.outcome,
            escape(&d.at),
            escape(&d.reason)
        );
    }
    for m in &view.merges {
        let _ = write!(
            out,
            "<p>Merge {} → {} · Reversed: {}<br>{}</p>",
            escape(&m.source),
            escape(&m.target),
            m.reversed,
            escape(&m.reason)
        );
    }
    out.push_str("<h2>Review history</h2>");
    for d in &view.decisions {
        let _ = write!(
            out,
            "<p>Record {} · {:?} · {}<br>{}</p>",
            escape(&d.target_id),
            d.state,
            escape(&d.at),
            escape(&d.reason)
        );
    }
    out.push_str("<h2>Observations</h2>");
    for o in &view.observations {
        let _ = write!(
            out,
            "<p id=\"{}\">{}: {} <a href=\"#{}\">Source</a> · {:?} · Entity <a href=\"#{}\">{}</a><br>Source anchor: {}</p>",
            escape(&o.id),
            escape(&o.field),
            escape(&o.value),
            escape(o.anchor.evidence_id()),
            o.review,
            escape(&o.entity_id),
            escape(&o.entity_id),
            escape(&serde_json::to_string(&o.anchor)?),
        );
    }
    out.push_str("<h2>Evidence register</h2>");
    for e in &view.evidence {
        let _=write!(out,"<section id=\"{}\"><h3>{}</h3><p>SHA-256: <code>{}</code><br>Origin group: {}<br>Extraction: {}<br>Imported: {}</p><pre style=\"white-space:pre-wrap\">{}</pre></section>",escape(&e.id),escape(&e.name),escape(&e.sha256),escape(&e.origin_group),escape(&e.extraction_status),escape(&e.imported_at),escape(e.text.as_deref().unwrap_or("No text derivative is available; consult retained original evidence.")));
    }
    out.push_str("<h2>Public web acquisition history</h2>");
    for e in &view.evidence {
        for capture in &e.acquisitions {
            let _ = write!(
                out,
                "<p>Source <code>{}</code>: {} · Retrieved {} · Job {}</p>",
                escape(&e.sha256),
                escape(&capture.url),
                escape(&capture.retrieved_at),
                escape(&capture.job_id)
            );
        }
    }
    out.push_str("<h2>Limitations and outstanding enquiries</h2><p>No automated identity conclusion or physical-presence inference is made. Geographic coverage, source independence and extraction completeness require explicit review. This HTML snapshot is self-contained and will not change when the workspace is corrected.</p></html>");
    Ok(out)
}
