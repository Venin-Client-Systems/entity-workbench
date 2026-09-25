/** Browser-test-only mount seam. It never overrides transport/availability or
 * provides an executor. Queueing here reaches the actual refusing standalone core. */
import { createRoot, type Root } from "react-dom/client";
import { command } from "../src/api";
import { DurableCollection } from "../src/DurableCollection";
import { DurableCollectionSession } from "../src/durable-collection-session";
import type { CollectionPreview } from "../src/durable-collection-types";
import type { Evidence } from "../src/types";
let session = new DurableCollectionSession();
let root: Root | null = null;
export function mount(revision: number, evidence: Evidence[]) {
  const node = document.createElement("div");
  node.id = "collection-test-mount";
  node.style.marginLeft = "280px";
  node.style.padding = "24px";
  document.body.append(node);
  root = createRoot(node);
  root.render(
    <DurableCollection
      revision={revision}
      evidence={evidence}
      session={session}
      busy={false}
      refresh={async () => false}
    />,
  );
}
export function unmount() {
  root?.unmount();
  root = null;
  document.getElementById("collection-test-mount")?.remove();
}
export async function initiateRefusedQueue(revision: number) {
  const preview = await command<CollectionPreview>({
    action: "preview_collection",
    input: {
      urls: ["https://example.org/research/"],
      max_hops: 2,
      max_requests: 50,
      max_seconds: 600,
    },
  });
  await session.queue(preview, revision, async () => false);
}
export function state() {
  return session.snapshot();
}

export async function useCanonicalRequest(
  key: string,
  urls: string[],
  revision: number,
) {
  session = new DurableCollectionSession(() => key);
  const preview = await command<CollectionPreview>({
    action: "preview_collection",
    input: { urls, max_hops: 2, max_requests: 50, max_seconds: 600 },
  });
  await session.queue(preview, revision, async () => false);
}
