# Native resource startup and public graph commands

One fixed native experiment passed on the available Apple Silicon Mac at clean
signed source `b680d4bcf6a6dbf2200ca5c455b4daf85024d983`. It used the actual
`start_with_development_app_resources` host constructor and the public graph
commands described by the [reviewed source contract](GRAPH-HOST-API-NATIVE.md).
No capability, verifier or worker executor was substituted in this native case.

The [exact observation](verification/native-graph-host-b680d4b.json),
[independent root audit](verification/native-graph-host-b680d4b-root.json) and
[integrated source checks](../verification/integration-graph-host-b680d4b.json)
retain the evidence identities. The actual release test executable, raw request,
wrapper, result, logs and synthetic workspace remain retained locally.

## What passed

The real constructor found and verified its fixed `engines/python` resource
child. Public `QueueGraphPath` admitted one request for `a` to `c`; the observer
recorded exactly one adapter call and one native child launch. Termination and
cleanup were confirmed before dependent post-execution reads.

`PageGraphJobs` returned the one completed job. `InspectGraphJob` supplied its
immutable result reference, whose exact ID and digests were passed to
`InspectGraphAnalysis`. The result was `a → b → c`, with both parallel accepted
assertions on the first hop and the accepted assertion on the second. Rejected,
pending and deferred shortcuts did not enter the path. Revisions were requested
2, queued 3, captured 4 and published 5.

Replaying the original request key and requested revision returned the same
completed inspection without another worker call or canonical write. Shutdown
joined; reopening returned the identical frozen result and original-integrity
status. Processing scratch was empty.

The seeded workspace had 25 records; completion had 28. The only new canonical
records were the request mapping, processing job and graph result. History
retained the queued and running job bodies, with exactly queue, claim and finish
events. The full schema, storage version, remaining records, derivative table,
sequences and original file matched the expected transition.

## Independent checks

Root matched all 1,388 tracked source files to their immutable Git blobs and
checked the retained test executable, receipt and exact raw graph bytes. A
separate read-only SQLite reconstruction matched every saved table and passed
the integrity check without changing the database. Independent breadth-first
search over the captured edges returned the same three-node path.

A second agent independently matched the immutable source, compiler artifact,
closed database, all 22 selected fingerprints, accepted-only topology, frozen
provenance, exact assignment bytes and both runtime inventories. No concrete
mismatch remained. This is agent review, not independent human release approval.

Both the original Python prefix and the copied resource prefix were independently
rehashed: 11,320 files and 601,821,300 bytes each, with manifest
`4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822`.
The inventory's historical `assembled-unexecuted` metadata is preserved from
assembly; this later execution is recorded by its separate native receipt.

The retained executable SHA-256 is
`690eb0eb5ef75a8a32e213685adda67771f189e869a6bd639fb87cd420507c88`;
the receipt SHA-256 is
`e3f60a5e22e0d9fe709382d9ce768ad6a9325986639c17e4d11273575131954d`.
The integrated ordinary suite passed 700 Rust tests, with 34 specialized cases
ignored. Strict host Clippy and formatting passed. Python ran 392 cases in each
normal and optimized mode: 388 passed and four platform checks skipped. The
source handoff separately retains strict release and Intel-target compilation.

## Limits

This was one invocation with no warm-up or automatic retry. Limits remained
600 seconds for the offline build, 300 seconds for the outer native test,
180 seconds for coordinator waiting, and 30 seconds for child wall/CPU time.
Startup inventory and other preparation are outside the child deadline; the
observation is not a performance guarantee.

This proves the host constructor and public command lifecycle on the development
Mac. It does not launch a Tauri application, establish packaged resource lookup,
enable ordinary graph startup, validate a graph interface, or prove signed
helpers and other supported platforms. All complete-release gates remain false.
