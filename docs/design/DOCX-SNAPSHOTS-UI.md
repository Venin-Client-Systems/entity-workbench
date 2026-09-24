# DOCX snapshot catalogue and native saving

The Assessment panel adds a separate editable-DOCX catalogue below the existing
HTML report list. It reuses the industrial panel, list-card, amber action button,
status, error and pager components documented in
[Assessment review](ASSESSMENT-REVIEW.md). Existing HTML snapshot bytes and export
behaviour are unchanged. The new DOCX-specific Figma extension and comparison
are being reviewed separately; this source change does not establish design
approval or complete report assembly/exhibit support.

The catalogue requests 20 records at one workspace revision. First, Back and
Next retain at most 100 cursor positions and use the number of actual rows
returned. An application-owned reader lane allows one active and one latest
pending request, including across revision changes and Assessment remounts.
Obsolete pages are hidden; a failed read is unavailable, never an empty result.
Pager focus returns to its status only when the analyst has not deliberately
moved focus while the request was pending.

Each row shows the captured source revision, retained timestamp, snapshot ID,
document and DOCX digests, DOCX byte count and template/generator identities.
This is a metadata catalogue. Listing does not rehash original sources or frozen
artifact bytes. The UI does not load the full frozen document model merely to
show the list.

Capture sends one caller-generated UUID and the visible source revision to
`save_docx_snapshot`. The canonical result is direct, followed by a separate
workspace refresh. If publication succeeds but refresh fails, the UI reports a
saved snapshot and offers a catalogue refresh. An uncertain acknowledgement
retains exactly the same UUID and captured revision through navigation and
remounting. Retry cannot silently replace either value. This state is held only
for the current application lifetime; reload/restart recovery is not implemented.

The current transport exposes untyped error messages. Consequently this first
UI slice conservatively treats a definitive source-revision rejection like an
uncertain acknowledgement. It offers same-request retry and does not infer
permission to start a replacement capture from error wording. A typed lookup
and analyst-authorized recovery flow is a follow-up; this error flow is not
claimed complete.

Native saving sends only the snapshot ID and both expected artifact digests.
Rust loads and verifies the retained document and DOCX, prepares a private
stage, and returns a typed ticket. The UI checks the receipt's snapshot ID,
source revision, both digests and byte count before committing. A late
preparation after the row unmounts is discarded. The existing same-ticket
commit retry can recover a lost acknowledgement without creating a second file.
Success displays the verified app-managed file location. DOCX bytes and arbitrary
filesystem paths are never supplied by the frontend, and no WebKit download is
used. Browser development explicitly requires the native application to save;
it does not offer a pretend DOCX download.

The browser tests use actual Rust commands and the existing
`native_export_session` harness with a synthetic workspace. They cover empty and
unavailable catalogue states, 43-record pagination, original IDs/order, revision
and navigation races, retained creation requests, refresh failure, corrupt
metadata, actual binary saving, lost commit acknowledgement, late stage discard,
substituted preparation metadata and corrupted frozen artifacts. The native
transport is a test bridge to Rust; these tests do not establish actual WebKit,
Windows or Intel Mac interaction.

The initial verification ran all nine new workflows and a combined 17-case set
including the existing assessment and native HTML/JSON exports. The production
TypeScript/Vite build passed. At 960 CSS pixels the catalogue had no horizontal
overflow and its axe scan reported no violations. Desktop, compact and native
save-receipt images were inspected locally. These checks establish browser
behaviour; the DOCX Figma comparison and actual native application checks remain
separate evidence.
