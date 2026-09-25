# Evidence local-index result consumption

The Evidence register retains the existing toolbar, source rows, source inspector, styling and industrial design foundation. This change repairs consumption of the existing `Search` response; it adds no controls, layout, CSS, backend contract or runtime activation.

The app retains the exact submitted query and the workspace revision observed when the search began. A private typed state distinguishes idle local filtering, pending index work, a validated response and failure. Query edits, navigation and revision changes advance a generation and clear the old result/error. Rendering also checks the current query/revision/context, including the render before layout-effect invalidation. Returning A → B → A cannot revive the old A request. Unmount invalidates outstanding work.

One app-lifetime read lane permits one dispatched command and one latest pending request. Invalidation removes queued work. It does not cancel or claim to stop an already running backend worker. A queued request is dispatched only if it remains current. Old completions cannot publish results/errors or clear the current pending state. Search pending/error is separate from shared workspace mutation busy/error, so a late search cannot enable controls while an import still awaits its reply.

The unchanged response has `workspace_revision` as a decimal string, `hits` and `total`. The client requires the exact observed revision; at most 100 hits; known unique canonical evidence IDs; names equal to the canonical rows for that revision; finite scores; and a nonnegative safe-integer reported total at least as large as the returned list. Unexpected or malformed fields refuse the entire response. No valid prefix is rendered. The client retains `total` as reported metadata without inventing a precision/relation claim. The existing displayed source count is the number of rows actually rendered.

Validated hit IDs are mapped to canonical Evidence rows **in returned order**. Source identity and inspection continue to use the canonical rows rather than worker-supplied names or text. Scores are neither re-ranked nor presented as confidence. No new source anchors or search receipt are invented. The response does not echo the query: query association is the client's captured request/generation, not an independently verified backend query receipt.

Starting an index attempt clears previous ranked hits immediately. Pending, failed, malformed and valid-empty responses show no ranked rows. An edit or navigation returns to the existing direct text-filter behavior; those local matches are not claimed as index results.

## Verification boundary

The scoped Playwright suite imports synthetic text evidence through the real Rust canonical API. Only index replies are explicitly injected to exercise reverse rank order, query/navigation/revision races, one-active/latest-pending behavior, new-attempt/failure clearing, malformed-reply refusal, and shared mutation busy protection. A delayed import reply is obtained from the actual core response. Wide and compact renders and accessibility observations use the existing surface.

These tests establish frontend consumption behavior. They do **not** execute Lucene, validate native index quality, prove total-hit precision, or supply frozen benchmark discovery/ranking evidence. No public request, native search run or normal collection activation is included. Existing design is reused; no new editable design frames are claimed.
