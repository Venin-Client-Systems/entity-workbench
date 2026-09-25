# Locked Python wheel plans for Intel Mac and Windows

These are metadata-only build inputs for EW-07. The fixed plans contain **58
Intel Mac** and **60 Windows x64** distributions. Windows adds `colorama` and
`tzdata` through the existing lock's platform markers. Both use the same six
direct package versions and transitive versions already in `workers/python/uv.lock`.
Development dependencies and the local virtual project are excluded.

| Target | Plan | Declared archive bytes |
| --- | --- | ---: |
| macOS Intel | [58 selected wheels](plans/python-macos-x86_64-wheels.v1.json) | 110,032,930 |
| Windows x64 | [60 selected wheels](plans/python-windows-x86_64-wheels.v1.json) | 102,969,632 |

These byte counts come from the lock, not downloaded or installed files. Every
plan entry binds name, version, exact wheel filename, official archive URL,
declared size and SHA-256 to an existing locked wheel. The Intel selection uses
the x86_64 DuckDB wheel rather than its universal2 alternative, and the NumPy
10.13-tagged wheel rather than the separately available 14.0 build. The existing
PyArrow wheel declares macOS 12.0. These tags guide artifact selection; they do
not establish the application's minimum supported OS or actual native loading.

`python3 scripts/verify_python_target_plans.py` checks both plans offline. Its
dependency walk supports only the two marker forms present in the pinned lock;
new markers or multiple locked versions require review. It rejects missing or
duplicate packages, development dependencies, altered versions/hashes/sizes,
unreviewed URLs and wheel tags. This is a finite reviewed contract, not a general
dependency or compatibility solver. Output includes each plan hash so later
staging can bind the exact reviewed selection. It does not admit these plans to
the existing arm64-only installer.

Independent `uv 0.11.32` frozen offline dry runs in an otherwise empty project
environment reported the same complete name/version sets. The invocation used
`uv sync --frozen --no-dev --no-install-project --dry-run --offline --no-build
--no-python-downloads --no-managed-python`, an explicitly selected installed
development CPython 3.13.11, and each exact `--python-platform` target. The
flags prevent synchronization, source builds and downloads; they are not a
claim that uv resolved using the candidate CPython 3.13.15 runtime. Astral's
[locking and syncing documentation](https://docs.astral.sh/uv/concepts/projects/sync/)
distinguishes the frozen lock from installation. The local CLI help supplied
the dry-run and cross-platform option semantics.

The dry-run log is an independent check of package names and versions, **not**
of the exact selected wheel filenames or native dependencies. An earlier
exploratory Intel dry run against an existing development environment reported
only its proposed delta; it is not used as the complete-set comparison. No
project environment or lock was changed. The qualified comparison uses the
fresh worktree, where no `.venv` was created. Five negative/selection tests run
normally and with optimization; [the observation](evidence/python-target-plans-2026-09-25.json)
binds the source and logs without publishing local paths.

No archive was downloaded, installed or executed by this work. The arm64 plan,
original runtime metadata, installed prefix and previous campaign evidence are
unchanged. Target-specific CPython archives, wheel verification/staging,
entry-point and notice review, native library closure, worker confinement,
relocation and actual Windows/Intel execution remain required. Source-CI hosts
do not replace clean installation tests of the final packages. No runtime
component or complete-release gate passes from this planning contract.
