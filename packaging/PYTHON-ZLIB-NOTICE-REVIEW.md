# CPython zlib notice metadata review

The pinned macOS arm64 CPython archive contains 19 licence files, but its
`PYTHON.json` also refers to an absent `licenses/LICENSE.zlib-ng.txt`.
The staging and installation receipts retain that discrepancy. This review
explains the upstream annotation mechanism; it does not declare the complete
distribution's notices approved or change any retained input bytes.

The inspected Python Build Standalone source is commit
`6a729962cddc76630b59b1b895501b1539412524`, associated with the selected
20260924 release. The installed `PYTHON.json` remains byte-identical to the
archive metadata, SHA-256
`1b11195bfd1066cf1d5037e577e98efbc120e55f2f92018a19b15ca3859d0c5b`.
Its `build_info.extensions.zlib[0]` describes one link with `name: "z"` and
`system: true`, while listing both zlib and zlib-ng licence paths.

In the upstream
[annotation function](https://github.com/astral-sh/python-build-standalone/blob/6a729962cddc76630b59b1b895501b1539412524/pythonbuild/utils.py#L558),
each link name is matched against every download's `library_names`. All matching
licence names and paths are accumulated. A system link is not excluded from
that matching. The separate `have_local_link` flag only controls an error when
no licence is known; it does not select which annotations are attached.

The same revision's
[download definitions](https://github.com/astral-sh/python-build-standalone/blob/6a729962cddc76630b59b1b895501b1539412524/pythonbuild/downloads.json)
give both zlib and zlib-ng the library name `z`, and associate them with distinct
notice filenames. Applying those definitions to the observed system link
explains why both paths appear in the metadata. This is an inference from the
exact source and candidate metadata, not a claim that the upstream build was
reproduced.

The earlier [static loader inspection](PYTHON-INSTALL-PREFLIGHT.md) also records
incoming `/usr/lib/libz.1.dylib` references in the CPython executable aliases,
libpython and `_tkinter`. That supports use of the OS zlib in those load paths.
It does not establish the absence of every possible statically incorporated
component, or settle the notices required by other bundled packages.

The retained upstream files have these identities:

| Source | Bytes | SHA-256 |
| --- | ---: | --- |
| `pythonbuild/utils.py` | 21,297 | `ae17589e90f41a4e8815e51f37decbd0ad32c6464c58fbc26c88f50e359752ec` |
| `pythonbuild/downloads.json` | 19,730 | `b9c25d10338ad220addc8a85d9c199d41af333dd1868076148eccfdbcf24a118` |
| `cpython-unix/extension-modules.yml` | 24,692 | `b8faaf74b8e4ac2daaf90b360660dc2e725305b2e241da985aec50543067e7c4` |

The archive, raw metadata, 19 actual notices, wheel provenance and installed
prefix stay unchanged. No unrelated notice is substituted and no annotation is
removed to make the inventory appear complete. The discrepancy is now
**explained at the metadata-generation level**, with final third-party notice
review still open. Existing failure flags remain truthful historical evidence;
this note grants no release gate or runtime component acceptance.
