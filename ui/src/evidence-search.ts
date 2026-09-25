import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { command } from "./api";
import { CitationReadLane as ReadLane } from "./citation-read-lane";
import type { Evidence } from "./types";

type SearchHit = { id: string; name: string; score: number };
type SearchReply = {
  workspace_revision: string;
  hits: SearchHit[];
  // Retained as reported metadata. The wire has no total precision/relation field.
  total: number;
};
type Request = { query: string; revision: number };
type SearchState =
  | { status: "idle" }
  | (Request & { status: "pending" })
  | (Request & { status: "ready"; reply: SearchReply })
  | (Request & { status: "failed"; error: string });

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function keys(value: Record<string, unknown>, expected: string[]) {
  return (
    Object.keys(value).length === expected.length &&
    expected.every((key) => Object.hasOwn(value, key))
  );
}
function readReply(
  value: unknown,
  revision: number,
  evidence: Evidence[],
): SearchReply {
  const invalid = () =>
    new Error(
      "Local index response is invalid or does not match the current evidence revision.",
    );
  if (
    !record(value) ||
    !keys(value, ["workspace_revision", "hits", "total"]) ||
    value.workspace_revision !== String(revision) ||
    !Array.isArray(value.hits) ||
    value.hits.length > 100 ||
    typeof value.total !== "number" ||
    !Number.isSafeInteger(value.total) ||
    value.total < value.hits.length
  )
    throw invalid();
  const canonical = new Map(evidence.map((item) => [item.id, item]));
  const seen = new Set<string>();
  const hits: SearchHit[] = [];
  for (const hit of value.hits) {
    if (
      !record(hit) ||
      !keys(hit, ["id", "name", "score"]) ||
      typeof hit.id !== "string" ||
      typeof hit.name !== "string" ||
      typeof hit.score !== "number" ||
      !Number.isFinite(hit.score) ||
      !canonical.has(hit.id) ||
      canonical.get(hit.id)!.name !== hit.name ||
      seen.has(hit.id)
    )
      throw invalid();
    seen.add(hit.id);
    hits.push({ id: hit.id, name: hit.name, score: hit.score });
  }
  return {
    workspace_revision: value.workspace_revision,
    hits,
    total: value.total,
  };
}

/** One app-lifetime read lane. Invalidation drops queued work and visible results;
 * it does not claim to cancel an already dispatched index worker. */
export function useEvidenceSearch(
  active: boolean,
  query: string,
  revision: number | undefined,
  evidence: Evidence[],
) {
  const [state, setState] = useState<SearchState>({ status: "idle" });
  const [lane] = useState(() => new ReadLane<unknown>());
  const generation = useRef(0);
  const invalidate = useCallback(() => {
    generation.current += 1;
    lane.clearPending();
    setState({ status: "idle" });
  }, [lane]);
  useLayoutEffect(() => {
    lane.open();
    return () => {
      generation.current += 1;
      lane.close();
    };
  }, [lane]);
  useLayoutEffect(() => {
    invalidate();
  }, [active, query, revision, invalidate]);

  const search = async () => {
    if (!active || revision === undefined || !query) return;
    const token = ++generation.current;
    const request = { query, revision };
    setState({ status: "pending", ...request });
    try {
      const value = await lane.read(() =>
        command<unknown>({ action: "search", query }),
      );
      if (generation.current !== token) return;
      const reply = readReply(value, revision, evidence);
      setState({ status: "ready", ...request, reply });
    } catch (error) {
      if (generation.current !== token) return;
      setState({ status: "failed", ...request, error: String(error) });
    }
  };
  // Context checks hide obsolete state in the render that precedes the layout
  // invalidation; A→B→A additionally advances generation on each transition.
  const current =
    active &&
    state.status !== "idle" &&
    state.query === query &&
    state.revision === revision;
  const byId = new Map(evidence.map((item) => [item.id, item]));
  return {
    search,
    invalidate,
    pending: current && state.status === "pending",
    error: current && state.status === "failed" ? state.error : "",
    rows: current
      ? state.status === "ready"
        ? state.reply.hits.map((hit) => byId.get(hit.id)!)
        : []
      : null,
  };
}
