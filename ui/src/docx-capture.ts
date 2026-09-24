import { command } from "./api";
import {
  isDocxSnapshot,
  validateDocxResolution,
  type DocxSnapshot,
  type DocxCaptureResolution,
} from "./docx-snapshot-types";
import type { DocxSnapshotPage } from "./docx-snapshot-types";
import { CitationReadLane } from "./citation-read-lane";

type Request = { id: string; revision: number };
type State = {
  request: Request | null;
  phase: "idle" | "creating" | "resolving" | "uncertain" | "saved";
  snapshot: DocxSnapshot | null;
  error: string;
  refreshFailed: boolean;
  resolution: DocxCaptureResolution | null;
  acknowledged: { request: Request; resolvedRevision: number }[];
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
    resolution: null,
    acknowledged: [],
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
  async resolve(minimumRevision: number, refresh: () => Promise<boolean>) {
    if (this.active || this.state.phase !== "uncertain" || !this.state.request)
      return;
    this.active = true;
    const request = this.state.request;
    this.publish({
      ...this.state,
      phase: "resolving",
      error: "",
      resolution: null,
      refreshFailed: false,
    });
    try {
      const result = await command<DocxCaptureResolution>({
        action: "resolve_docx_capture",
        request_id: request.id,
        captured_revision: request.revision,
      });
      validateDocxResolution(
        result,
        request.id,
        request.revision,
        minimumRevision,
      );
      this.publish({
        ...this.state,
        phase: result.outcome.state === "saved" ? "saved" : "uncertain",
        snapshot:
          result.outcome.state === "saved" ? result.outcome.snapshot : null,
        resolution: result,
      });
      const refreshed = await refresh().catch(() => false);
      this.publish({ ...this.state, refreshFailed: !refreshed });
    } catch (cause) {
      this.publish({
        ...this.state,
        phase: "uncertain",
        error: String(cause),
        resolution: null,
      });
    } finally {
      this.active = false;
    }
  }
  async startNew(revision: number, refresh: () => Promise<boolean>) {
    const { request, resolution } = this.state;
    if (
      this.active ||
      !request ||
      resolution?.outcome.state !== "not_recorded" ||
      resolution.workspace_revision <= request.revision ||
      revision < resolution.workspace_revision
    )
      return;
    // A distinct analyst action acknowledges this proven absence. Never turn a
    // lookup, refresh or lost acknowledgement into an automatic replacement.
    const acknowledged = [
      ...this.state.acknowledged,
      { request, resolvedRevision: resolution.workspace_revision },
    ].slice(-20);
    this.publish({
      ...this.state,
      phase: "idle",
      request: null,
      resolution: null,
      acknowledged,
    });
    await this.capture(revision, refresh);
  }
  async capture(revision: number, refresh: () => Promise<boolean>) {
    if (this.active) return;
    this.active = true;
    const request =
      this.state.phase === "uncertain" && this.state.request
        ? this.state.request
        : { id: crypto.randomUUID(), revision };
    this.publish({
      ...this.state,
      request,
      phase: "creating",
      snapshot: null,
      error: "",
      refreshFailed: false,
      resolution: null,
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
        ...this.state,
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
        ...this.state,
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
