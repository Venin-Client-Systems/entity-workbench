# Transaction review completion ownership

A successful correction or review closes only the selection instance that
submitted it. If the analyst closes that review and opens the same or another
transaction while the acknowledgement is pending, the old callback cannot close
the later selection. The later draft remains visible at its captured revision;
an arriving correction makes it stale and disables decisions until a verified
current row is reopened. It is never silently rebased onto the changed data.

The complete browser campaign first exposed a late-close race while reopening a
corrected transaction. That campaign retains 118 passes and one failure. Two
additional regressions hold the actual Rust correction acknowledgement, close
the submitting review, open a same-row or different-row review, and enter a new
draft before releasing the response. Both failed before the repair because the
new review disappeared. Both pass afterward, preserving the draft and stale
state, with exactly one canonical correction and no accepted decision.

The original sequential investigation test now waits for its submitted review
to finish before reopening. That explicit sequencing does not replace the two
adversarial delayed-response regressions. All 15 targeted tests pass: the two
new lifecycle cases, the two broad investigation workflows and all 11 existing
summary/ledger workflows. The production build passes. These are real-core
browser observations, not installed native-platform acceptance.

This changes completion ownership within the existing designed review surface;
it introduces no new visual layout, public command or schema.
