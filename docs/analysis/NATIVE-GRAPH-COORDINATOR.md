# Native graph coordinator campaign

One four-case campaign passed on the available Apple Silicon Mac from clean source
`9fb28f8f85d1be96da95f007ef274c7725535a7b`. It exercised the real `JobCoordinator`,
canonical SQLite workspaces, fixed packaged Python/NetworkX adapter and macOS
development confinement profile. It did not enable graph execution at normal
application startup.

The [retained verification record](../verification/native-graph-coordinator-9fb28f8.json)
binds the exact source tree, executable, parent observations, requests, results,
canonical snapshots and original evidence. All fixtures are synthetic. Raw
workspaces and runtime copies remain private; the public record contains their
identities and scoped observations.

## Observed results

| Case | Result | Canonical revisions |
| --- | --- | --- |
| Ordinary path and publication retry | `a → b → c`; both parallel first-hop assertions retained. An injected publication failure kept the same result and lease for retry, with no second worker call or launch. | Requested 2; queued 3; captured 6; published 7. Competing jobs then advanced the workspace to 13. |
| Unreachable destination | Immutable `unreachable` result for `a → f`. | Requested 2; queued 3; captured 4; published 5. |
| Larger canonical graph | 700 entities, 699 accepted assertions and a 700-node path; actual request 83,746 bytes and result 27,741 bytes. | Requested 2; queued 3; captured 4; published 5. |
| Live-child cancellation | `cancelled_by_analyst`; no graph record or wrapper/result bytes. | Requested 2; queued 3; captured 4; cancellation 5; terminal 6. |

Each case recorded exactly one adapter call and one child launch, with distinct
assignment IDs, capture nonces and PIDs. All four recorded confirmed termination
and cleanup. The ordinary case held document and collection competitors at zero
calls during pending publication; each then executed once and reached its expected
synthetic Blocked state. This checks scheduling resumption, not successful document
parsing or online collection.

Cancellation followed a host observation of a live child: observer-relative times
were 4,701 ms for live observation, 4,731 ms for caller cancellation and 4,762 ms
for confirmed stop. This does not identify the child's NetworkX computation phase
or prove it was still alive at the exact signal instruction. These times are not
performance measurements.

## Retained checks

Root independently matched all 1,318 tracked source files against the immutable
Git tree and verified the actual release test executable. A read-only check of
each closed database passed SQLite integrity checking and the strict complete-table
receipt validator. The canonical snapshots bind schema, metadata, records,
history, events, derivative objects and SQLite sequences. Original evidence bytes
were unchanged. Independent breadth-first traversal reproduced the two paths and
the unreachable result; scratch directories were empty. A second agent reviewed
the retained campaign and found no concrete mismatch. This is agent review, not
independent human release approval.

The original and fresh app-local Python prefixes each contained 11,320 files,
601,821,300 bytes and 58 locked wheels. The runner verified both before use and
after all children were known stopped. Successful adapter deliveries also verified
their prefix after execution; the cancelled adapter deliberately performed no
post-inventory read. No native retry, warm-up, timeout increase or dependency
download occurred.

The fixed limits were 600 seconds for the offline source build, 900 seconds for
the ordered campaign, 180 seconds for a coordinator wait, 30 seconds for child
wall/CPU limits and five seconds for the live-child handshake. A larger fixture
is capacity evidence for these exact bytes, not the required 16 GB benchmark.

## Remaining work

Normal graph activation, public commands and designed controls remain separate
implementation work. This campaign does not establish signed helper confinement,
Windows or Intel Mac execution, hard RSS enforcement, installed-artifact behaviour
or general release readiness. All twelve complete-release gates remain unpassed.
The separate combined spaCy compatibility timeout remains unresolved.
