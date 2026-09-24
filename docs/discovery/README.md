# Direct collection discovery benchmark v1

This is the measurement foundation for [EW-05 / issue #9](https://github.com/Venin-Client-Systems/entity-workbench/issues/9). **No live benchmark has run.** The scorer, synthetic replays and frozen task definitions do not establish useful web discovery or pass a release gate.

`public-benchmark.v1.json` fixes 30 harmless tasks, ten independently named publisher groups and ten official-domain seed choices. There are ten organisation, ten name and ten domain inputs. Names refer to public programmes, facilities or collection objects; there are no private people, case records or client information. Questions and relevance criteria are original benchmark metadata, not copied website content. These public references belong in `docs/discovery`; automated fixtures remain entirely fictional under `fixtures/discovery`.

The seed URLs are proposed collection starting points. They have **not** been fetched or approved for automated access as part of this implementation. Inclusion says nothing about current availability, robots rules, website terms, retention permission or permission to reproduce content. A permitted, useful operational path remains unproven.

## Frozen scope and interpretation

The task file is frozen before first measurement. Record its SHA-256, repository commit and review decision in every run. The scorer binds results to its exact bytes and refuses changes to the v1 denominator, limits or proposed thresholds. A changed task set, input, seed, threshold or scoring rule requires a new version and a recorded product decision. Never replace unsuccessful tasks after observing results.

Each task starts from its listed publisher's seed in a **new empty local corpus**. The analyst previews and selects that domain and the lower applicable limits. Do not pre-load target pages, hand-enter answer URLs, borrow another task's cache/index or add a more convenient seed during measurement. The initial request must use the frozen seed; later content requests must descend from prior responses. Access-review requests cannot become scored discovery sources.

The collector fetches selected public websites directly, and the application indexes and searches the acquired corpus locally. There is no search API, metasearch service, hosted index, account, key or remote operator. The benchmark search phrase stays local. Its current URL contract allows HTTPS GET/HEAD, normal HTTPS ports, no credentials, no query parameters, no fragments, and only the selected publisher domain and its subdomains. A redirect outside that scope is blocked and recorded. A future broader policy needs its own reviewed benchmark version.

Collection stops at the first applicable limit: **two expansion hops, 50 request attempts or ten minutes**, including access/robots requests, redirects, retries and failed attempts. Website-specific lower limits override these ceilings. Redirects retain hop depth but consume requests; following a content link increases depth by one. Requests within a task are serial. The scorer rejects a trace that exceeds its declared effective limits; actual enforcement must be demonstrated by the Rust broker and application tests.

All 30 tasks must be attempted in one dated campaign, within 14 days of the first attempt. Their corpus-reset receipts, acquisition dates and review dates are retained. Later campaigns start with empty corpora again and remain separate; do not pool each task's best result across runs. Collection that cannot proceed lawfully or technically is a measured blocked or failed outcome, not grounds to omit a task.

## Access and disclosure record

Before each task, review the applicable robots rules, site terms and collection/retention/display constraints. Use the same application broker for any related network requests and include them in the task budget. Record:

- The reviewer, review date, direct publisher/domain, seed and permitted request methods.
- The URLs, retrieval dates and hashes of the applicable robots/terms material; allow/deny/unknown decisions and reasons. Unknown permission is not treated as approval.
- Effective request, time and hop limits, rate constraints and any requested attribution, retention or export conditions.
- The exact local search phrase, permitted URL scope, fixed User-Agent and other sent headers. Do not send cookies, credentials, local reference numbers or case contents.
- The actual ordered request URLs, methods, response status, redirect/link parent, timestamps and body hashes. There are no externally submitted search queries in v1; URLs and headers are still disclosures.

An access-review artifact is mandatory even when no content request can proceed. Hash verification establishes artifact integrity, not the correctness of the recorded access decision. A reviewer must inspect its contents. Do not publish third-party page bodies merely to make benchmark evidence public; retain originals locally according to the recorded conditions, and publish only a reviewed, sanitised receipt or permitted excerpt.

## Relevance and expansion labels

The acquisition operator does not label their own results. A separate reviewer uses the task's frozen `relevance_criterion` and `expansion_criterion`, examines the acquired originals, follows source anchors and checks what the application actually displayed. Reviewer identifiers must differ from the runner identifier; organisational identity and actual independence are reviewed outside this script.

`relevant_result` is true only when a successful acquired source answers the task question under its criterion. A search hit, matching word, inaccessible snippet, redirect or unrelated namesake is insufficient. Source references point to successful GET bodies, retained by hash, which also appear in the local search and UI evidence.

`useful_expansion` requires a relevant result and a documented chain: the first acquired source reveals a new identifier; that identifier leads through the collector to a distinct, further relevant source at a later hop. Record the identifier, its exact source anchor, the ordered request references and the further lead's anchor. The trace must link the lead back to the identifier source within the task limits. Two preselected pages or a second copy of the first page are not a useful expansion. Semantic relevance and copied-source checks require human review; hashes and chain shape cannot decide them.

Labels include a rationale, independent reviewer identifier and review time. If the reviewer is unavailable or the evidence is ambiguous, leave `labels` as `null`. Retain disagreements and their adjudication in the evidence review record; do not silently convert an uncertain label to success. Review every attempted task, including blocked/no-result/failed tasks. Sanitised runner/reviewer IDs are sufficient in public receipts; do not publish account identity or private paths.

## Measures and proposed minimums

The denominator is always **30**, including blocked, quota-exhausted, failed and no-result attempts. The proposed minimums are **18 relevant tasks (60%)** and **9 useful expansions (30%)**. At least one source-to-identifier-to-further-lead chain must be inspectable inside the actual application. The numerical expansion minimum is stronger than that single demonstrator.

| State | Treatment |
| --- | --- |
| No run supplied or no task attempted | `not_run`; yields are `null`, not 0%. |
| Omitted task or explicit `not_run` result | Remains in the denominator; entire campaign is incomplete. |
| Attempted task awaiting independent labels | Costs retained, label missing, campaign incomplete. |
| Successful retrieval with irrelevant results | Measured attempt, zero relevant/expansion credit. |
| Successful local query with no results | `no_results`; measured zero credit. |
| Access denied or scope blocked | `blocked`; measured zero credit, retain exact reason. |
| Source quota or an exhausted job limit | `quota_exhausted`; measured zero credit, retain limit and reason. |
| Technical execution failure | `failed`; measured zero credit and failure evidence. |

The scorer reports status counts, task IDs missing measurements or labels, request totals, total task time and maximum task costs. Percentages are `null` until all 30 attempts have independent labels. An incomplete campaign cannot pass by counting only completed tasks. Threshold failure is a product finding, not a reason to lower a gate.

This is a deliberately bounded **selected-source** experiment. The ten institutions and programme/object names do not represent arbitrary people, the entire web, unknown websites, poorly linked pages or authenticated/dynamic content. A domain never seeded or reached by a permitted link has no local coverage. Failure to find it does not establish that it does not exist. Even a successful run cannot justify an exhaustive-web or general identity-search claim. Record inaccessible pages, missing domains and input classes separately in the Sprint 1 product assessment.

## Offline tools

No command below makes a network request or launches the application.

```sh
# Validate the frozen public task file; expected exit 1 and not_run.
python3 scripts/discovery_benchmark.py

# Write fabricated evidence to a NEW ignored directory, then score it.
python3 fixtures/discovery/make_synthetic_replay.py --output artifacts/discovery-synthetic
python3 scripts/discovery_benchmark.py \
  --benchmark artifacts/discovery-synthetic/benchmark.json \
  --run artifacts/discovery-synthetic/run.json \
  --evidence-root artifacts/discovery-synthetic/evidence

python3 -m unittest discover -s scripts/tests -p 'test_discovery_benchmark.py' -v
```

The synthetic replay reaches exactly 18/30 relevant and 9/30 expanded tasks to exercise threshold boundaries. Those values are fabricated test expectations. Its `live_measurement_eligible` is always false. Generated text labelled `ui_capture` is a synthetic stand-in for application evidence, not an actual screenshot.

CLI exit status is 0 for a structurally valid complete campaign meeting the numerical thresholds, 1 for a valid unmeasured/incomplete/below-threshold campaign, and 2 for malformed, unsafe, mismatched or corrupt evidence. **Exit 0 alone is never a release signal:** a synthetic campaign also uses it to test the scorer. Consumers must inspect mode and `live_measurement_eligible`, then independently review actual acquisition, access decisions, UI evidence and source semantics. `release_gate_decision` is always `not_evaluated`.

See [the evidence format](RUN-FORMAT.md) for the exact run contract. The scorer reads bounded local files and validates trace/evidence consistency. It is not the network broker, a hostile-file sandbox, an authenticity signer, or a substitute for the application's acquisition integration. Hashes cannot prove that a receipt was produced by the stated application revision. Only score immutable, locally staged evidence; do not allow another process to mutate its directories during verification.

## Remaining EW-05 work

1. Independently review this task set, publisher independence, criteria and fixed thresholds before first live measurement; record the review against the frozen digest.
2. Review each publisher's current access constraints through the authorised application path. Do not infer automated-access permission from public visibility.
3. Connect actual Rust broker/job receipts, cold-corpus resets, original hashes, local-search output and application views to the run format. The operator must demonstrate actual in-app results; an external browser session does not satisfy it.
4. Attempt every task under the fixed limits. Preserve the original campaign, collect independent labels and run the offline scorer.
5. Review source semantics, disclosure records, all chains, blocked cases and coverage limitations. Publish a sanitised measured report with its input and evidence hashes.
6. If usefulness or permitted collection cannot satisfy the thresholds, record the concrete product/architectural blocker by Sprint 1's end, retain the failed campaign and continue independent local features. Leave the complete-release gate open.
