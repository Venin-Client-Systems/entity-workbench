# Explicit development graph startup

The desktop has a default-off Cargo feature, `development-graph-runtime`. An
explicit build with `--features development-graph-runtime` selects the new native
host constructor `JobCoordinator::start_with_development_app_resources`. Ordinary
desktop builds and `JobCoordinator::start` continue to configure graph execution
as unavailable. The feature does not stage or install a runtime.

The desktop reads Tauri's application resource directory once. Existing document
engines use its `engines` child; graph configuration derives only
`engines/python`. There is no command, frontend field, setting, environment
variable or PATH lookup for selecting the graph runtime. Relative resource roots
are refused as unavailable rather than resolved against the current directory.
The constructor is a trusted native host API, not a deserializable execution
capability. It cannot accept a custom verifier or worker recipe.

On macOS arm64, configuration uses the existing opaque `VerifiedGraphRuntime`
constructor and exact full-prefix inventory: 11,320 files, manifest
`4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822`.
No interpreter, package, plugin or worker runs during this verification. Other
platforms remain unsupported even when the development feature is enabled.
Missing, invalid or unsupported resources leave graph availability at
`runtime_unavailable` while the normal review/document coordinator starts.
Typed cancellation is preserved before and after preflight; it aborts startup.
Unexpected bootstrap errors are not parsed or converted into optional absence.

The verified capability is installed before coordinator threads start. There is
no live runtime attachment or race in which queued jobs first see an unavailable
executor and later gain one. Existing recovery and execution ownership checks
still control startup: retained search uncertainty or unverified processing exit
overrides runtime availability with `recovery_required`. The default document
executor is shared by both startup constructors without behavior changes.

`ready` means that this host has configured the fixed capability and currently
holds execution authority. It is not proof of successful future execution or a
permanent integrity guarantee. Each graph attempt retains existing runtime
revalidation, private workspace-derived scratch, exact captured input and result
binding, exclusive scheduling, cancellation, termination verification, cleanup,
and atomic canonical publication. `Workspace::open` already prepares its scratch
directory with mode 0700 on Unix; the adapter independently rechecks ownership,
mode and linked ancestors before creating an assignment. Bootstrap adds no
permission changes or alternate scratch path.

## Packaging and evidence boundaries

The existing macOS Tauri resource mapping includes `runtime/staged/engines` as
`engines`. A Python directory is not inferred to exist merely because this feature
compiles. A separate explicit development packaging step can use the already
reviewed `scripts/relocate_python_prefix.py` with the original verified prefix and
a fresh `runtime/staged/engines/python` destination. It must retain the exact
manifest, no-clobber behavior and independent source/destination verification.
No copying, download, installation, console wrapper or package hook is performed
at application startup. This source slice does not stage that directory.

Inventory verification reads approximately 602 MB before startup completes.
Configuration is synchronous, so the development window may appear only after
verification. This is not a startup latency guarantee. The existing 30-second
child execution deadline does not include configuration, graph capture,
preparation or final inventory checks; capture can verify up to 1,000 originals
of 64 MiB each while holding the workspace transaction. No timeout or resource
limit is increased here.

Source tests exercise fixed child derivation, missing/invalid/linked resource
refusal, relative-root refusal, preflight cancellation, typed error preservation,
unchanged default behavior, document admission with an absent document runtime,
workspace scratch derivation and quarantine dominance. They execute no native
worker. A synthetic available graph executor tests shared quarantine precedence;
it does not fabricate a verified Python capability. The unsupported-platform
regression is compiled on Intel macOS; cross-compilation is not native execution.

An actual packaged desktop graph campaign remains a separate finite experiment
after source review. Prior fixed and coordinator-native graph observations stay
bound to their original source and fixtures; they do not prove this desktop
bootstrap. Minimum OS, distribution signing, complete notices, supervisor-crash
containment and release acceptance remain separate requirements. No schema,
canonical graph protocol or public command changes are part of this increment.

## Source verification

The focused seven host bootstrap regressions and full 698-test ordinary core
suite passed; 33 native tests remained ignored. Strict all-target core Clippy
passed for the arm64 host and the Intel macOS cross target. The ordinary Python
source suite ran 364 tests, with 360 passing and four platform skips. All 110
pre-existing files under `schemas` remain byte-identical to the integration base.

The initial desktop source check stopped in Tauri's build script because this
fresh worktree has no `runtime/staged/engines` directory. That failure is retained.
Strict all-target desktop Clippy passed with the feature off and on for both
the arm64 host and Intel macOS cross target, using the explicit build-only
`TAURI_CONFIG='{"bundle":{"resources":[]}}'` override, leaving the committed
configuration unchanged. These checks cover feature branches without bundled
resource validation; they do not establish a working packaged application.
