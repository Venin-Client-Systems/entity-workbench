# Packaged graph capability startup on the development Mac

One isolated Tauri `.app` built and started successfully at clean signed source
`0cf52f720259890f1395275f271fc8d94486e65c`, tree
`d3d10040e48fb43f33a699b7176c32d84a4e16c1`. The experiment ran on the available
Apple Silicon Mac, macOS 26.6.2 build 25G83. It used the explicit
[startup-proof feature](GRAPH-PACKAGED-STARTUP-PROOF.md), a fresh compiled
identifier and a previously absent app-local workspace.

The [observation](verification/packaged-graph-startup-0cf52f7.json) and
[independent root audit](verification/packaged-graph-startup-0cf52f7-root.json)
bind the actual source, configuration, executable, runtime and closed workspace.
This is an unsigned development startup observation. No graph job was submitted
in this experiment; the earlier [native host/public-command experiment](NATIVE-GRAPH-HOST-RESULTS.md)
remains the separately identified worker-execution evidence.

## Build and real resource lookup

The pinned Tauri CLI built one application with `--no-sign`, the proof feature,
and offline, locked Cargo arguments. It used a fresh target directory and a
fresh no-clobber copy of the reviewed Python prefix. The exact configuration
removed the inherited resource mapping before adding only that prefix. The
merged mapping, unchanged configuration bytes and resulting `Info.plist` were
checked; no unrelated engine staging entered this package.

The build completed with exit zero in 313.07 seconds. The actual bundled
`Contents/MacOS/entity-workbench` executable is 15,181,104 bytes, SHA-256
`d70f5dc6bd3b009252f56796c53e58b19c1ed593439afe6ad956be35c5d0f27c`.
Before launch, the complete bundled Python prefix passed verification against
manifest `4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822`:
11,320 files and 601,821,300 bytes. The source and copied prefixes matched too.

The actual application derived its paths through Tauri, reserved its new local
directory exclusively and invoked the resource-backed coordinator constructor.
Its public typed graph catalogue reported `ready`, revision zero, zero jobs and
no continuation. The fixed startup receipt also recorded main-window creation
and the Finished callback for the bundled document.

## Window and normal shutdown

Root observed the actual native accessibility tree and screenshot through CUA.
The rendered Overview showed **REV 0**, zero evidence items, entities,
transactions to review and collection gaps, with the synthetic-example button
available. The document URL was `tauri://localhost`. The graphite navigation,
square amber controls and empty workspace instruments were visible. This was
an operator observation, not independent human design approval. The screenshot
remains conversation evidence; no separate PNG export or image-file hash is
claimed.

Root issued normal Command-Q. CUA reported that the app quit, and the owned OS
process was reaped with exit zero after 44.38 seconds including observation
time. This duration is not a startup-latency measurement. The exact event
sequence was constructor readiness, bundled-window readiness and one joined
shutdown with the empty revision-zero catalogue unchanged. No retry, seed,
import or graph submission occurred.

The parent retained OS-exit evidence before interpreting diagnostics. Acceptance
used a capped diagnostic read, duplicate-key rejection and exact event/schema/
details validation. The 1,800-second build and 300-second app wait deadlines were
not exhausted. Timeout handling separately allows up to ten seconds for TERM
and ten seconds for KILL/reap; these are not hard total elapsed-time guarantees.

## Closed-state checks

After confirmed process exit, root independently checked all 1,401 tracked
source blobs and the complete 11,323-file application inventory. Every bundle
file remained byte- and mode-identical to its prelaunch record. Independent
walkers rehashed all three complete Python prefixes, checked ordinary
single-linked files, executable modes and the exact inventory. Their original
assembly-time `assembled-unexecuted` metadata remains unchanged; later native
observations are recorded separately.

A read-only, immutable SQLite inspection matched the full schema and all six
tables against an independently constructed fresh schema-five baseline. The
workspace remained at revision zero. Records, history, events, derivative
objects and sequences were empty; originals, scratch, backups, exports and
native export staging were empty. The database passed integrity and foreign-key
checks and remained byte-identical throughout inspection. Its SHA-256 is
`c898b6ce7a514633dba266aacaa86b6d440b02644dbf8b8ed227d3d12da880a0`.
The processing lock could be reacquired without writing it.

A second agent independently matched the immutable source, reviewed drivers,
configuration and real `Info.plist`, every bundle file, all three runtime
inventories including 58 package RECORDs each, the raw receipt sequence,
confirmed process exit and the full closed canonical state. No concrete
mismatch remained. This was agent artifact review, not independent human
release approval.

The [root desktop integration checks](../verification/integration-packaged-startup.json)
passed eleven source tests with the feature off and eleven with it on, strict
proof-feature Clippy and formatting. The earlier
[source handoff](verification/packaged-graph-startup-source.json) retains six
strict ARM/Intel configurations and the repaired Windows test-path finding.
Those source checks used an empty-resource build override; the actual package
observation above supplies separate resource evidence.

## Remaining work

This proves one actual development package's resource lookup, rendered empty
window and joined shutdown on the available Mac. It does not prove a graph
submission from that package, a graph interface, default graph activation,
complete dependency packaging, clean offline installation, application-wide
network silence, minimum-OS compatibility, other-platform execution, signed
helpers, distribution signing or notarization. All twelve complete-release
gates remain false.
