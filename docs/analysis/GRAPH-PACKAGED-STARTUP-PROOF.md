# Isolated packaged graph startup proof

This source increment adds an explicit, default-off desktop feature,
`development-graph-startup-proof`, which implies `development-graph-runtime`.
It prepares a startup-only experiment. It has not built or launched an app,
staged a runtime, invoked an interpreter, queued a graph job, or demonstrated
packaged worker execution. Ordinary desktop startup and its IPC handlers are
unchanged when this feature is absent. No command or schema is added.

## Isolation before workspace initialization

The feature requires macOS arm64 and a compiled Tauri identifier of exactly
`org.entityworkbench.graphstartupproof.<canonical-lowercase-UUID>`. It derives
app-local data and resource paths from Tauri, and the executable from Tauri's
platform helper. There is no environment variable, runtime path field, frontend
capability, default identifier, or workspace selector for the proof.

Before any workspace creation, the helper refuses existing app-local data,
linked or missing ancestors, nonabsolute paths, a local data leaf that differs
from the compiled identifier, and an executable/resource pair outside the same
`.app/Contents/MacOS` and `.app/Contents/Resources` layout. This deliberately
refuses Cargo's ordinary target directory and an earlier proof workspace.
An exclusive directory creation reserves the absent app-local root, with Unix
mode 0700. A concurrent launch that creates it first is refused; no existing
directory is adopted or removed. A failed subsequent startup retains its new
directory for diagnosis and needs a fresh identifier for another experiment.
These checks do not assert protection against an actor replacing ordinary
ancestor directories concurrently.

Only then does the existing `Workspace::open` initialize
`workspaces/default`. A fresh workspace has revision zero, current schema five,
empty canonical tables and no originals. Initialization also creates private
working directories; coordinator startup creates its ownership lock and native
export housekeeping directories. These are expected filesystem effects, not
evidence of canonical mutation. The proof never seeds data or opens an existing
workspace. Existing migration/recovery logic therefore has no previous data to
adopt. The parent experiment must independently inspect all canonical tables,
schema, sequences and originals after the application has closed.

The helper attaches the same document runtime and calls the actual
`start_with_development_app_resources` constructor with Tauri's resource root.
It then calls the public `PageGraphJobs` dispatcher with expected revision zero
and page size one. Only typed schema-one `ready`, revision zero, count zero,
empty rows and no continuation pass. Missing or invalid Python resources and
unsupported hosts cannot be mistaken for available capability. The configured
capability still uses the exact existing inventory, fixed worker, supervision
and lifecycle rules; no interpreter runs during configuration.

## Fixed receipt and shutdown

Feature-only stderr lines start with `GRAPH_STARTUP_PROOF` and contain JSON with
`schema_version: 1`, `kind: development_graph_startup_proof`, an event, and
bounded fixed-label details. They contain no paths, identifiers, URLs, arbitrary
error bodies or user data. Events are:

- `constructor_ready`: `resource_location: app_bundle`, `availability: ready`,
  `workspace_revision: 0`, `graph_job_count: 0`.
- `bundled_window_ready`: native window creation and the bundled document's
  Finished callback have both occurred for the `main` window.
- `window_observation_refused` or `startup_refused`: explicit fixed-stage failure.
  An invalid window observation remains failed even after a later valid callback.
- `shutdown`: one-shot booleans `joined`, `unchanged_empty_graph_catalogue`,
  `bundled_window_ready`, and their conjunction `passed`.

Page/window observations may arrive in either order. No script or frontend
acknowledgement fabricates readiness. A window creation plus Finished callback
proves this lifecycle boundary, not successful React rendering, visible pixels,
accessibility, future worker execution, or a startup latency bound. The parent
must retain actual window readiness evidence separately. The app does not
automatically exit: the operator closes it normally through the existing app
lifecycle. Shutdown reads the empty catalogue before stopping the coordinator,
then records the result of the existing joined shutdown. That catalogue read
does not establish full database/original conservation. Repeated ExitRequested
and Exit notifications do not create a second proof result.

## Locked page origin

The checked local sources are Tauri 2.11.6, tauri-utils 2.9.3 and Wry 0.55.1.
`WebviewUrl::default` is `App(index.html)`. The committed window configuration
does not override it, and `frontendDist` is the local `../ui/dist` directory.
Tauri's `manager/webview.rs::prepare_webview` omits the `index.html` path and
uses `manager/mod.rs::get_app_url`; for a packaged directory that calls
`tauri_protocol_url`, which returns `tauri://localhost` on macOS. Wry's
`wkwebview/class/wry_navigation_delegate.rs` passes the actual webview URL with
its Finished notification. The proof accepts only that URL, with an optional
trailing slash. Dev-server origins, alternate documents, query strings and
other window labels are refused. This source determination is not an observed
native load; an unexpected native URL will fail explicitly rather than widen
the accepted origin.

## Future isolated packaging, not performed here

Use a fresh private worktree/build directory, a fresh compiled proof identifier
and a separate product name. The actual runtime must be copied to a fresh
`runtime/staged/engines/python` with the reviewed no-clobber relocation tool;
the source prefix must remain untouched. Keep the fixed manifest
`4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822`.
The existing `desktop/tauri.macos.conf.json` maps this tree to
`Contents/Resources/engines/python`. Do not stage unrelated engines for this
experiment. An isolated external source can instead be mapped explicitly while
deleting the inherited resource entry in the same config patch:

```json
{"bundle":{"resources":{
  "../runtime/staged/engines/":null,
  "../isolated-proof/python/":"engines/python/"
}}}
```

Replace the synthetic source spelling with the reviewed isolated source. The
final map must have exactly one entry. The pinned
[CLI config loader](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.5/crates/tauri-cli/src/helpers/config.rs)
retains explicit nulls when combining command-line patches, then applies JSON
Merge Patch to base plus platform config. Locked tauri-build/codegen apply the
same patch. A new map alone retains the old map entry. A separate array reset
followed by a new map is also unsuitable: composition can discard the reset
before merging with the inherited config. A source regression uses the locked
Tauri config reader to prove naive addition retains two entries and explicit
key removal yields one, without reading any resource tree.

Review the exact merged config and resulting Info.plist so the
compiled identifier, fresh app-local root and bundle identity agree.

The installed CLI is pinned to 2.11.5. Its local `build --help` exposes
`--no-sign`. The corresponding [build source](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.5/crates/tauri-cli/src/build.rs)
and [bundle source](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.5/crates/tauri-cli/src/bundle.rs)
forward that flag to the bundler. In the pinned
[macOS app bundler](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.5/crates/tauri-bundler/src/bundle/macos/app.rs),
resources are copied before a `no_sign` branch which skips keychain handling,
signing, attribute removal and notarization. The separate
[signing helper](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.5/crates/tauri-bundler/src/bundle/macos/sign.rs)
has no automatic ad-hoc identity when no identity/certificate is supplied.
Using the explicit flag avoids ambient signing configuration for this experiment.

A future invocation from `desktop` can therefore use the installed CLI with
`build --no-sign --bundles app --features development-graph-startup-proof
--config <reviewed-isolated-config> -- --offline --locked`. This command has
not been run by this source slice. `--no-bundle` is unsuitable: it cannot prove
the required resource layout. Do not run recursive ad-hoc signing over embedded
Python Mach-O files: their exact bytes are pinned. No verifier exception or
manifest regeneration is permitted to make packaging pass. Independently verify
the actual bundled Python prefix before launch and after exit, including file
bytes and executable modes; source staging equality alone is insufficient.
Linker signing of the newly built main executable is a separate identity and
does not establish bundle distribution signing or notarization.

## Source verification and limits

Eleven macOS source regressions cover canonical proof identifiers, read-only refusal of
default/existing/linked/nonbundle locations, exclusive fresh-directory creation,
typed catalogue readiness, one-shot event ordering, wrong page/window refusal,
the locked document/resource configuration, and an actual empty ordinary coordinator
remaining unavailable. They run with proof features off and on, using inert
synthetic filesystem fixtures only. No fabricated verified runtime is used.

Desktop test and strict all-target Clippy checks use
`TAURI_CONFIG='{"bundle":{"active":false,"resources":[]}}'`; this explicit
source-check override excludes packaging and resources. Host arm64 checks and
Intel cross checks cover default, runtime-only and proof branches. Cross-compilation does not
execute Intel code, and compiling the proof does not verify a packaged runtime.
The first test build's fixture syntax failure and a parallel fixture-name
collision are retained alongside the final logs in the scoped verification
manifest. A per-process atomic sequence now distinguishes fixture directories
even when the system clock returns identical timestamps. No dependency,
lockfile, historical schema, UI or core production source is changed.

Peer review identified that Windows CI also builds desktop unit tests: absolute
Windows paths contain a prefix that the proof deliberately refuses. The positive
POSIX bundle tests are therefore Unix-only. A Windows-only regression explicitly
requires path refusal without workspace creation; portable identifier/readiness
and ordinary-coordinator cases remain enabled. Production path admission and
the unsupported-host refusal were not widened. A Windows GNU cross-Clippy
attempt stopped in the existing C dependency build because the MinGW C compiler
was unavailable. That log is retained; it is not a successful Windows compile.
Actual Windows execution still requires its hosted source check.

Actual packaged startup, visible-window observation, complete closed canonical
and original conservation, runtime inventory after packaging/exit, and joined
native shutdown remain pending. Packaged graph execution, distribution signing,
notarization, minimum-OS validation and release acceptance are separate gates.
