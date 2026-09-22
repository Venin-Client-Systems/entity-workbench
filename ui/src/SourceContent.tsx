import { useEffect, useState } from "react";
import { command } from "./api";
import type { Anchor, Evidence, SourceExcerpt } from "./types";

export function SourceContent({
  evidence,
  anchor,
}: {
  evidence: Evidence;
  anchor?: Anchor;
}) {
  const [excerpt, setExcerpt] = useState<SourceExcerpt | null>(null),
    [error, setError] = useState("");
  const [start, setStart] = useState(
    anchor?.kind === "text" ? Math.max(0, (anchor.line_start ?? 1) - 1) : 0,
  );
  useEffect(() => {
    let current = true;
    if (anchor)
      void command<SourceExcerpt>({ action: "inspect_source", anchor })
        .then((result) => {
          if (current) setExcerpt(result);
        })
        .catch((error) => {
          if (current) setError(String(error));
        });
    return () => {
      current = false;
    };
  }, [anchor]);
  const lines = evidence.text ? evidence.text.split(/\r?\n/) : [];
  if (lines.at(-1) === "") lines.pop();
  const end = Math.min(start + 200, lines.length);
  return (
    <>
      {anchor && (
        <section className="disclosure" aria-label="Source anchor excerpt">
          {error ? (
            <p role="alert">{error}</p>
          ) : excerpt ? (
            <>
              <h3>{excerpt.location}</h3>
              <pre className="source-quote">{excerpt.quote}</pre>
              {excerpt.truncated && <p>Excerpt limited to 8,000 characters.</p>}
              <p className="muted">
                Validated against workspace revision{" "}
                {excerpt.workspace_revision}.
              </p>
            </>
          ) : (
            <p role="status">Resolving source anchor…</p>
          )}
        </section>
      )}
      {lines.length ? (
        <>
          <p className="muted">
            Retained text derivative · lines {start + 1}–{end} of {lines.length}
            . The original file is retained separately under its SHA-256.
          </p>
          <ol className="source-lines" start={start + 1}>
            {lines.slice(start, end).map((line, i) => (
              <li key={start + i}>{line || " "}</li>
            ))}
          </ol>
          <div className="actions">
            <button
              className="button"
              disabled={start === 0}
              onClick={() => setStart(Math.max(0, start - 200))}
            >
              Previous source lines
            </button>
            <button
              className="button"
              disabled={end === lines.length}
              onClick={() => setStart(end)}
            >
              Next source lines
            </button>
          </div>
        </>
      ) : (
        <p>
          No text derivative is available. The original is retained in the
          content-addressed evidence store.
        </p>
      )}
    </>
  );
}
