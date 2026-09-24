# Bounded assessment citation selection

The citation reader increment retains the existing industrial [finding-editor frame 25:398](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=25-398) and [finding-review frame 25:361](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=25-361). The [assessment design handoff](ASSESSMENT-REVIEW.md) remains its foundation: a 920 px dialog, graphite and amber palette, square controls, bordered citation rows and separate supporting/contradictory sections. New pagination, read-failure and stale-selection states do not yet have a verified editable Figma extension. The original unsynced tab was not touched. Implementation screenshots do not replace that remaining design-tool work.

## Canonical metadata readers

`AssessmentWorkbench` obtains citation metadata through command v17's `page_citation_catalogue` and `read_citation_selections`. It no longer builds citation rows from full transaction, observation or entity arrays. This changes the consumer, not the default workspace response: those arrays are still returned to other existing workflows. The [backend contract](../assessment/CITATION-CATALOGUE.md) defines exact field projection, canonical kind/sequence order, size bounds and source-identity checks.

Available citations request up to 50 rows and report the actual returned position range and complete matching count after selected-ID exclusions. First/previous/next navigation retains at most 100 prior positions. Byte-short pages advance by their returned row count. A successful zero-count response is distinct from an unavailable catalogue. An unavailable catalogue does not prevent saving a completely resolved selected set.

Search retains literal whitespace and punctuation. Input is debounced for 250 ms; old results are hidden while another query waits to apply. Queries exceeding 256 UTF-8 bytes are not submitted or truncated. Matching uses the server's reported whole-string lowercase algorithm and runtime Unicode version; the UI does not claim every browser's case mapping is identical. Query, exclusion set and revision changes reset continuation.

Each open editor has one catalogue request lane and one selected-set lane. Each lane permits one active command and one replaceable latest pending request. Applied queries or selected sets arriving during a read replace pending work; intermediate pending reads are rejected locally before dispatch. Closing the modal discards pending work. It does not claim to cancel an already dispatched core operation. Mounted/generation guards separately prevent an obsolete response from changing visible data. Reopening creates a new modal lifetime; this is not a global worker quota or a substitute for backend resource limits.

The selected reader resolves the complete union of role IDs, at most 100. Editor rows remain in catalogue order; review sections map the complete result into each saved supporting/contradicting ID order. Role changes preserve the existing remove-and-append semantics. Filter changes do not remove selected IDs. Empty selection is handled locally without sending an invalid zero-ID request.

## Drafts, failure and source semantics

The finding editor captures its mutation revision once. Workspace refresh preserves title, assessment, limitations, question choices, reason and citation roles without adopting a new mutation revision. A known revision change disables citation changes and saving, explains the stale state and leaves the draft visible until the analyst explicitly closes it. Backend conflicts also preserve the draft. The existing finding-review reason follows the same immutable-revision rule.

Missing, ambiguous, oversized or invalid selected metadata yields no partial set. The UI keeps every selected ID and role, permits explicit removal of an unavailable ID from a current draft, offers retry, and blocks save or review until the remaining selected set resolves. Failed catalogue reads do not imply zero matches. Origin groups are counted only from a complete selected result; sources without a recorded group are identified as unknown. A different group is not evidence of independence.

Rows distinguish observation, transaction and whole-source citations. Decimal strings, account leading zeros, review states and source metadata are preserved. Text, cell, page, message and capture anchor labels retain their type; displaying an anchor is not accepting it or proving the source viewer supports its position.

Nested source inspection still needs the full retained `Evidence.text` and acquisition history. Its existing evidence row is looked up by ID, digest and byte length; absent metadata matches disable inspection rather than inventing empty source content. This lookup is not an original-file rehash. Anchored inspection continues through the separate core source reader and exposes its own errors. Whole-source inspection remains the existing retained-text view. Source text is escaped. Citation metadata does not establish analyst acceptance, original integrity or source independence.

Prior report snapshots remain unchanged. Export still reads the selected immutable snapshot with its expected digest rather than rebuilding a previous report from current citation metadata. The new catalogue does not alter review history.

## Interaction and verification

Pager controls remain mounted while loading. User-triggered navigation/retry returns focus to its result status when focus has not moved elsewhere. Moving an available citation into the selected set follows its role control once complete metadata arrives; delayed completion does not steal focus from another field. The existing modal handles containment, nested source return and finding-opener restoration. Chromium consumes the first Escape in a nonempty search input to clear that input; modal Escape is checked separately.

Real-core tests use canonical synthetic import, observation/transaction review and finding operations. Delayed-response cases hold actual successful core responses; failure cases abort transport. No successful API payload is fabricated. Coverage includes multi-page counts, literal Unicode/whitespace search, inert markup, selection persistence, exact saved role order, source excerpts, immutable exports, unavailable versus empty states, retry focus, stale draft preservation, A → B → A replies, request coalescing, pending work discarded on close, and last-citation removal.

The initial citation handoff's full browser suite passed **79/79**, including all nine new citation scenarios. The production UI type/build and development core build passed; the existing large JavaScript chunk warning remains. Axe reported zero automated violations at 1440 and 720 px, with incomplete manual checks retained. The first new run passed 6/7: the compact test expected Escape to close a nonempty search input. A diagnostic confirmed Chromium instead clears the query; explicit query-clearing and subsequent modal-close assertions passed. Request-lane regressions were then added and all nine passed before that full run.

Subsequent native inspection reported that refreshing an open finding editor lost focus when the form disabled itself. A real delayed-core-response regression reproduced the failure in Chromium: the refresh button stayed inactive after it was re-enabled. The repair waits for both local refreshing and application busy state to finish, then restores the stable button in a cancellable animation frame only when focus stayed with the opener or fell back to the body/document element. It records deliberate focus movement, so later blur does not cause an unexpected return. Cleanup cancels pending focus work on unmount. The draft's revision, assessment, reason and citation roles remain unchanged.

The focused follow-up passed the production type/build and **21/21 citation, assessment and history workflows**, including three delayed real-response focus cases. The original reproduction retained one failure and one pass; the first repaired run passed 20/20 before the third focus case was added. This follow-up did not rerun the entire application suite or native WebKit. The [focused readout](review/citation-picker/refresh-focus-verification.json) keeps these observations separate from the earlier 79-test run and its stored visual evidence.

| State | Implementation evidence |
| --- | --- |
| Populated final page | [1440 px](review/citation-picker/citations-page-1440.png), [720 px](review/citation-picker/citations-page-720.png) |
| Successful zero-match query with selected citations retained | [1440 px](review/citation-picker/citations-1440.png), [720 px](review/citation-picker/citations-720.png) |
| Selected metadata unavailable, IDs and roles retained | [Unavailable state](review/citation-picker/citations-unavailable.png) |
| Stale draft and disabled mutation | [Stale state](review/citation-picker/citations-stale.png) |

These are scrolled implementation views within the existing modal and its bounded citation list. They are not whole-document renders, pixel-equivalence checks or owner sign-off. [Checksums](review/citation-picker/checksums.json) bind the saved evidence and source files. Final results and artifact identities are recorded in [the verification readout](review/citation-picker/verification.json). Native Tauri interaction, manual screen-reader checks, a new editable Figma extension and complete distribution/performance gates remain separate.
