import { command } from "./api";
import { CitationReadLane } from "./citation-read-lane";
import {
  validateInspection,
  isCanonicalUuid,
  type CollectionInspection,
  type CollectionRunPage,
  type CollectionPreview,
} from "./durable-collection-types";
type Pending = {
  key: string;
  preview: CollectionPreview;
  minimumRevision: number;
};
type State = {
  phase: "idle" | "queueing" | "uncertain" | "saved";
  pending: Pending | null;
  result: CollectionInspection | null;
  error: string;
  refreshFailed: boolean;
};
/** App-lifetime request identity; navigation cannot replace a lost acknowledgement.
 * No reload/restart persistence or inferred absence from error text. */
export class DurableCollectionSession {
  constructor(
    private readonly newRequestKey: () => string = () => crypto.randomUUID(),
  ) {}
  readonly catalogue = new CitationReadLane<CollectionRunPage>();
  readonly inspection = new CitationReadLane<CollectionInspection>();
  readonly previews = new CitationReadLane<CollectionPreview>();
  private state: State = {
    phase: "idle",
    pending: null,
    result: null,
    error: "",
    refreshFailed: false,
  };
  private active = false;
  private listeners = new Set<() => void>();
  snapshot = () => this.state;
  subscribe = (fn: () => void) => {
    this.listeners.add(fn);
    return () => {
      this.listeners.delete(fn);
    };
  };
  private publish(next: State) {
    this.state = next;
    this.listeners.forEach((fn) => fn());
  }
  async queue(
    preview: CollectionPreview | null,
    revision: number,
    refresh: () => Promise<boolean>,
  ) {
    if (this.active) return;
    const pending =
      this.state.phase === "uncertain"
        ? this.state.pending
        : preview
          ? { key: this.newRequestKey(), preview, minimumRevision: revision }
          : null;
    if (!pending) return;
    if (!isCanonicalUuid(pending.key))
      throw new Error("Collection request identity is invalid.");
    this.active = true;
    this.publish({
      phase: "queueing",
      pending,
      result: null,
      error: "",
      refreshFailed: false,
    });
    try {
      const result = await command<CollectionInspection>({
        action: "queue_collection",
        input: pending.preview.input,
        preview_sha256: pending.preview.preview_sha256,
        request_key: pending.key,
      });
      validateInspection(result, null, pending.minimumRevision);
      if (
        result.run.request_key !== pending.key ||
        result.run.record_version !== 4 ||
        result.run.collector_policy !== pending.preview.collector_policy ||
        JSON.stringify(result.run.input) !==
          JSON.stringify(pending.preview.input)
      )
        throw new Error(
          "Collection acknowledgement does not match the reviewed scope.",
        );
      this.publish({ ...this.state, phase: "saved", result });
      const ok = await refresh().catch(() => false);
      this.publish({ ...this.state, refreshFailed: !ok });
    } catch (cause) {
      this.publish({ ...this.state, phase: "uncertain", error: String(cause) });
    } finally {
      this.active = false;
    }
  }
}
