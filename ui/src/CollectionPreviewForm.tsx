import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { command } from "./api";
import { DurableCollectionSession } from "./durable-collection-session";
import {
  availabilityLabels,
  canQueue,
  validatePreview,
  type CollectionAvailability,
  type CollectionPreview,
} from "./durable-collection-types";
export function CollectionForm({
  revision,
  busy,
  session,
  refresh,
  availability,
}: {
  revision: number;
  busy: boolean;
  session: DurableCollectionSession;
  refresh: () => Promise<boolean>;
  availability: CollectionAvailability | null;
}) {
  const saved = useSyncExternalStore(session.subscribe, session.snapshot);
  const [urls, setUrls] = useState("https://example.com/"),
    [hops, setHops] = useState("2"),
    [requests, setRequests] = useState("50"),
    [seconds, setSeconds] = useState("600");
  const [preview, setPreview] = useState<CollectionPreview | null>(null),
    [error, setError] = useState(""),
    [loading, setLoading] = useState(false);
  const epoch = useRef(0),
    mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      epoch.current++;
    };
  }, []);
  function edit(change: () => void) {
    change();
    epoch.current++;
    setPreview(null);
    setError("");
    setLoading(false);
    session.previews.clearPending();
  }
  async function inspect() {
    const attempt = ++epoch.current;
    setLoading(true);
    setPreview(null);
    setError("");
    try {
      const result = await session.previews.read(() =>
        command<CollectionPreview>({
          action: "preview_collection",
          input: {
            urls: urls.split("\n").filter((s) => s.trim().length > 0),
            max_hops: Number(hops),
            max_requests: Number(requests),
            max_seconds: Number(seconds),
          },
        }),
      );
      validatePreview(result);
      if (mounted.current && attempt === epoch.current) setPreview(result);
    } catch (e) {
      if (mounted.current && attempt === epoch.current) setError(String(e));
    } finally {
      if (mounted.current && attempt === epoch.current) setLoading(false);
    }
  }
  const pending = saved.phase === "queueing" || saved.phase === "uncertain";
  return (
    <section className="panel" aria-label="Direct public-web collection">
      <h2>Direct public-web collection</h2>
      <p>
        Select websites and limits, then review the exact disclosure. Preview
        makes no DNS lookup or external request.
      </p>
      <p className="alert" role="status">
        {availability
          ? availabilityLabels[availability]
          : "Collection availability has not been confirmed. Preview is available; queueing is disabled."}
      </p>
      {pending ? (
        <div className="collection-pending">
          <h3>
            {saved.phase === "queueing"
              ? "Queue acknowledgement pending"
              : "Queue completion is unconfirmed"}
          </h3>
          {saved.error && (
            <p role="alert" className="alert error">
              {saved.error}
            </p>
          )}
          <p>
            Request <code>{saved.pending!.key}</code>
          </p>
          <p>
            Retry retains the same reviewed URLs, limits and disclosure
            identity. No absence has been established and no replacement is
            created. This request is retained only while the application remains
            open; reload or restart loses it.
          </p>
          <PreviewScope preview={saved.pending!.preview} />
          <button
            className="button primary"
            disabled={busy || saved.phase === "queueing"}
            onClick={() => void session.queue(null, revision, refresh)}
          >
            Retry same collection
          </button>
        </div>
      ) : (
        <>
          <label>
            Seed URLs, one per line
            <textarea
              aria-label="Seed URLs"
              maxLength={41000}
              value={urls}
              onChange={(e) => edit(() => setUrls(e.target.value))}
            />
          </label>
          <div className="durable-limits">
            <label>
              Expansion hops
              <input
                aria-label="Expansion hops"
                type="number"
                min="0"
                max="2"
                value={hops}
                onChange={(e) => edit(() => setHops(e.target.value))}
              />
            </label>
            <label>
              Maximum requests
              <input
                aria-label="Maximum requests"
                type="number"
                min="1"
                max="50"
                value={requests}
                onChange={(e) => edit(() => setRequests(e.target.value))}
              />
            </label>
            <label>
              Maximum seconds
              <input
                aria-label="Maximum seconds"
                type="number"
                min="1"
                max="600"
                value={seconds}
                onChange={(e) => edit(() => setSeconds(e.target.value))}
              />
            </label>
          </div>
          <button
            className="button"
            disabled={busy || loading || !hops || !requests || !seconds}
            onClick={() => void inspect()}
          >
            {loading ? "Reviewing scope…" : "Preview disclosure"}
          </button>
          <p className="muted">
            Changing any URL or limit requires a new preview.
          </p>
          {error && (
            <p role="alert" className="alert error">
              Preview unavailable: {error}
            </p>
          )}
          {preview && (
            <div className="collection-preview">
              <h3>Reviewed scope</h3>
              <PreviewScope preview={preview} />
              <button
                className="button primary"
                disabled={busy || !canQueue(availability)}
                onClick={() => void session.queue(preview, revision, refresh)}
              >
                Queue reviewed collection
              </button>
              {!canQueue(availability) && (
                <p>
                  Queueing is unavailable in the current execution state.
                  Previewing does not send a request.
                </p>
              )}
            </div>
          )}
        </>
      )}
    </section>
  );
}
function PreviewScope({ preview }: { preview: CollectionPreview }) {
  return (
    <>
      <h4>Selected URLs</h4>
      <ul>
        {preview.input.urls.map((url, i) => (
          <li key={i}>
            <code>{url}</code>
          </li>
        ))}
      </ul>
      <div className="durable-limits">
        <div>
          <h4>Selected hosts</h4>
          {preview.selected_hosts.map((h) => (
            <p key={h}>
              <code>{h}</code>
            </p>
          ))}
        </div>
        <div>
          <h4>Robots access</h4>
          {preview.robots_urls.map((u) => (
            <p key={u}>
              <code>{u}</code>
            </p>
          ))}
        </div>
      </div>
      <p>
        <strong>
          {preview.input.max_hops} hops · {preview.input.max_requests} charged
          requests · {preview.input.max_seconds} seconds
        </strong>
      </p>
      <p>
        Disclosed: hostnames for DNS, connection metadata, and the selected or
        followed URLs. Links and redirects stay on selected hosts. Robots access
        and redirects use the request budget.
      </p>
      <p>
        Local case contents are not submitted automatically. Check URLs for
        information you do not intend to disclose. Public access and robots
        permission do not establish a right to republish. Unsafe observed
        addresses or redirects can still be blocked.
      </p>
      <dl>
        <dt>Disclosure identity</dt>
        <dd>
          <code>{preview.preview_sha256}</code>
        </dd>
      </dl>
    </>
  );
}
