import { useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import type { Evidence } from "./types";
import { DurableCollectionSession } from "./durable-collection-session";
import {
  runLabels,
  type CollectionAvailability,
} from "./durable-collection-types";
import { CollectionForm } from "./CollectionPreviewForm";
import { RunCatalogue } from "./CollectionRunCatalogue";
import { RunReview } from "./CollectionRunReview";
import "./durable-collection.css";
export function DurableCollection({
  revision,
  busy,
  session,
  refresh,
  evidence,
}: {
  revision: number;
  busy: boolean;
  session: DurableCollectionSession;
  refresh: () => Promise<boolean>;
  evidence: Evidence[];
}) {
  const [availability, setAvailability] = useState<{
    revision: number;
    value: CollectionAvailability;
  } | null>(null);
  const [selected, setSelected] = useState<string | null>(null),
    [reload, setReload] = useState(0),
    [refreshError, setRefreshError] = useState("");
  const mounted = useRef(true);
  const saved = useSyncExternalStore(session.subscribe, session.snapshot);
  useLayoutEffect(() => {
    mounted.current = true;
    session.catalogue.open();
    session.previews.open();
    session.inspection.open();
    return () => {
      mounted.current = false;
      session.catalogue.close();
      session.previews.close();
      session.inspection.close();
    };
  }, [session]);
  async function refreshAll() {
    setRefreshError("");
    const ok = await refresh().catch(() => false);
    if (!mounted.current) return;
    if (ok) setReload((n) => n + 1);
    else
      setRefreshError(
        "Workspace refresh failed. The collection catalogue has not been revalidated.",
      );
  }
  return (
    <div className="durable-collection">
      <CollectionForm
        revision={revision}
        busy={busy}
        session={session}
        refresh={refresh}
        availability={
          availability?.revision === revision ? availability.value : null
        }
      />
      {saved.phase === "saved" && saved.result && (
        <div className="alert" role="status">
          Collection recorded: <code>{saved.result.run.id}</code> ·{" "}
          {runLabels[saved.result.run.state]}.{" "}
          {saved.refreshFailed &&
            "Workspace refresh failed; the collection remains recorded."}{" "}
          <button
            className="button"
            onClick={() => setSelected(saved.result!.run.id)}
          >
            Review recorded collection
          </button>
        </div>
      )}
      <section className="panel" aria-label="Durable collection catalogue">
        <div className="panel-heading">
          <h2>Durable collections</h2>
          <button
            className="button"
            disabled={busy}
            onClick={() => void refreshAll()}
          >
            Refresh collections
          </button>
        </div>
        <p>
          Recorded outcomes remain separate from current execution. Historical
          acquisition receipts are listed below.
        </p>
        {refreshError && (
          <p className="alert error" role="alert">
            {refreshError}
          </p>
        )}
        <RunCatalogue
          key={`${revision}:${reload}`}
          revision={revision}
          session={session}
          onAvailability={setAvailability}
          onSelect={setSelected}
        />
      </section>
      {selected && (
        <RunReview
          key={`${selected}:${revision}`}
          id={selected}
          revision={revision}
          session={session}
          evidence={evidence}
          onClose={() => setSelected(null)}
          refresh={refresh}
        />
      )}
    </div>
  );
}
