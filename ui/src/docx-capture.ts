import { command } from "./api";
import { isDocxSnapshot, type DocxSnapshot } from "./docx-snapshot-types";
import type { DocxSnapshotPage } from "./docx-snapshot-types";
import { CitationReadLane } from "./citation-read-lane";

type Request = { id: string; revision: number };
type State = {
  request: Request | null;
  phase: "idle" | "creating" | "uncertain" | "saved";
  snapshot: DocxSnapshot | null;
  error: string;
  refreshFailed: boolean;
};
/** Owned by App, not the Assessment panel. Navigation cannot lose an uncertain
 * request or its captured revision. Reload/restart recovery is not provided. */
export class DocxCapture {
  readonly catalogue = new CitationReadLane<DocxSnapshotPage>();
  private state: State = {
    request: null,
    phase: "idle",
    snapshot: null,
    error: "",
    refreshFailed: false,
  };
  private listeners = new Set<() => void>();
  private active = false;
  snapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private publish(state: State) {
    this.state = state;
    this.listeners.forEach((listener) => listener());
  }
  async capture(revision: number, refresh: () => Promise<boolean>) {
    if (this.active) return;
    this.active = true;
    const request =
      this.state.phase === "uncertain" && this.state.request
        ? this.state.request
        : { id: crypto.randomUUID(), revision };
    this.publish({
      request,
      phase: "creating",
      snapshot: null,
      error: "",
      refreshFailed: false,
    });
    try {
      const result = await command<unknown>({
        action: "save_docx_snapshot",
        request_id: request.id,
        expected_revision: request.revision,
      });
      if (
        !isDocxSnapshot(result) ||
        result.id !== request.id ||
        result.workspace_revision !== request.revision
      )
        throw new Error(
          "DOCX creation acknowledgement did not match the retained request.",
        );
      // Publication is confirmed before the separate refresh. Refresh failure
      // cannot change this into a failed capture or silently repeat the write.
      this.publish({
        request,
        phase: "saved",
        snapshot: result,
        error: "",
        refreshFailed: false,
      });
      const refreshed = await refresh().catch(() => false);
      this.publish({ ...this.state, refreshFailed: !refreshed });
    } catch (cause) {
      this.publish({
        request,
        phase: "uncertain",
        snapshot: null,
        error: String(cause),
        refreshFailed: false,
      });
    } finally {
      this.active = false;
    }
  }
}
