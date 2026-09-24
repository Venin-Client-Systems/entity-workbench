# Release access and decision readiness

[EW-08 / issue #12](https://github.com/Venin-Client-Systems/entity-workbench/issues/12) remains open. As assessed on **24 September 2026**, the record contains **8 unknown and 3 unverified requirements; none is confirmed or known missing**. This work establishes the readiness register and its evidence rules. It does not establish signing access, resource reservations or release approval.

The [versioned register](register.v1.json) records each requirement, responsible role, dates, required evidence and next action. The roles are proposed responsibilities awaiting assignment, not claims that somebody has accepted them. The [initial audit](audit-2026-09-24.md) records repository configuration and a narrow local environment check against source revision `67e84c6`.

| Requirement | Current status | Responsible role to confirm | Needed by | Later checkpoint |
|---|---|---|---|---|
| Mac Developer ID and notarization access | Unknown | Release signing custodian | 6 Oct | Signing rehearsal: 20 Oct |
| Windows signing access | Unknown | Release signing custodian | 6 Oct | Signing rehearsal: 20 Oct |
| Mac signing/notarization rehearsal | Unknown | Release engineer | 20 Oct | Exact candidate: 15 Dec |
| Windows signing rehearsal | Unknown | Release engineer | 20 Oct | Exact candidate: 15 Dec |
| Clean Windows 11 x64 environment | Unknown | Platform test operator | 30 Sep | Downloaded-artifact offline test: 15 Dec |
| Clean Apple Silicon environment | Unverified | Platform test operator | 30 Sep | Downloaded-artifact offline test: 15 Dec |
| Clean Intel Mac environment | Unknown | Platform test operator | 30 Sep | Downloaded-artifact offline test: 15 Dec |
| 16 GB benchmark environment | Unverified | Performance test operator | 6 Oct | Required workload measurement: 15 Dec |
| Supported minimum OS/build matrix | Unverified | Product owner and platform maintainer | 6 Oct | Every advertised boundary tested: 15 Dec |
| Maintainer review capacity | Unknown | Release maintainer | 6 Oct | Candidate acceptance review: 15 Dec |
| Security review capacity | Unknown | Security reviewer | 6 Oct | Candidate security acceptance: 15 Dec |

All dates are 2026 programme calendar dates. The 30 September environment target gives time to run and repair confinement probes before Sprint 1 ends on 6 October. These intermediate dates are agent scheduling choices within the approved programme. They are not commitments from an unassigned operator. Signing rehearsals are due by Sprint 2 exit; final downloaded-artifact testing is due by Sprint 6 exit. The 16–23 December buffer remains for repairs, retesting and final sign-off.

## What the statuses mean

- **Unknown:** availability or the decision has not been established. Absence of a repository secret, signing setting or record cannot prove that an account or machine is missing.
- **Unverified:** relevant partial evidence exists, but the requirement's required evidence is not satisfied. The development Mac and source-build matrix are partial evidence only.
- **Confirmed:** an assigned responsible role has accepted the scope, and a dated sanitized confirmation establishes the specific requirement. Signing rehearsals need actual verification results. Confirmation of access does not pass any product release gate.
- **Missing:** an explicit dated availability check establishes that a required resource or commitment is unavailable. A failed check needs its result and next action; silence or an expired deadline does not become “missing.”

A missed date is reported separately as **overdue**. Each confirmation has an explicit inclusive `valid_until` date. Readiness expires at that date, the required later checkpoint or programme end, whichever comes first. Confirmation recorded on or after the later checkpoint can renew readiness through its declared validity date, never past programme end. A historical `confirmed` status remains visible, but an expired confirmation appears in `stale_confirmations`, `unresolved` and `overdue` and cannot make the register ready. Reconfirm reservations and signing access at the checkpoint; revoked access must be updated immediately.

## Complete the access checks

1. The product owner nominates the signing custodian, platform/performance operators, maintainer and security reviewer. Record acceptance of each role and review window. Public records contain roles and sanitized commitments; private identities and access instructions remain outside this repository.
2. The signing custodian confirms the permitted Mac and Windows signing routes privately. Record usable availability, intended release scope and renewal/revocation responsibility. Never publish key material, certificate/account identifiers, tokens, recovery information, identity documents or private locations. No account, purchase or security-setting change is authorized by this checklist.
3. Operators reserve each target and record a reset procedure, supported OS/architecture, absence of developer prerequisites in the clean snapshot, and the ability to block/observe application network traffic. Provide useful sanitized environment facts; exclude machine names, serials, addresses and personal accounts. Run the platform issue's benign/hostile probes during Sprint 1. The source-build runners do not satisfy installed product tests.
4. The product owner and platform maintainer choose the minimum OS/build ranges using runtime and confinement results. Windows 11 x64 and both Mac architectures are required; specific supported versions/builds and test boundaries need a recorded decision. A Tauri default, hosted runner image or current development OS is not an owner decision. Update packaging metadata and advertised support together when the matrix is settled.
5. The release engineer rehearses the full signing path by 20 October. Retain candidate hashes, sanitized verification output and architecture/OS-specific launch results. Mac results must cover bundled helpers as well as the outer app/package. The rehearsal is separate from final candidate signing and confinement acceptance.
6. The maintainers reserve review capacity before release-candidate construction. Follow existing required reviews without self-approval. Security review covers the threat model, confinement, broker/network disclosures, hostile inputs, dependencies/licences and recovery. Record unresolved findings and repair dates. Final acceptance binds exact candidate artifact hashes; a general willingness to review is access readiness only.

The observed arm64 development host has 16 GiB of physical memory. A performance operator must still decide whether it can be reserved and run the specified workload reproducibly. Record CPU/storage classes, OS, workload, power/thermal conditions, concurrency, cache state, repetitions, p95 query/filter latency, OCR/import throughput and peak memory. Do not substitute a small synthetic timing for the programme's 100,000 transactions and 10,000 pages, including 1,000 scanned pages with concurrent map/graph use.

## Maintain the record without overstating evidence

Retain prior audit/attestation documents. Add a new sanitized Markdown evidence record when a role accepts a responsibility, access is confirmed/unavailable, a decision is made or a rehearsal runs. Include observation date, scope, method, result, limitations and responsible role. Bind rehearsal evidence to candidate hashes and sanitized verification results. Review the document's content before publishing; a checksum cannot establish the truth of an attestation.

Add its repository-relative path, SHA-256 and recorded date to the `evidence` table. Use `inspection`, `confirmation`, `unavailability` or `role_assignment` as appropriate. Confirmation records require an inclusive `valid_until` date supported by the attestation, no earlier than the observation and no later than programme end. Other evidence kinds use `valid_until: null`. Retain old confirmations when adding renewal evidence; do not extend an old attestation by changing its validity date without supporting evidence. Update each requirement's assessment date and evidence references, and update the register assessment date consistently. `confirmed` needs confirmation evidence plus a separately documented role assignment; `missing` needs unavailability evidence. Unknown/unverified requirements have no confirmation references. A specific operator may fulfill multiple roles if the independent review requirements remain satisfied.

The offline checker rejects missing requirements, invalid status transitions, absent/changed evidence files, duplicate JSON keys and inconsistent dates. It rejects symlinks and Windows junction/reparse points at the repository root and within referenced public paths. Unexpected file/parse errors return a generic diagnostic without local paths. It also reports unresolved requirements, overdue obligations and stale confirmations. It reads only local public files and does not fetch URLs or examine credentials. It validates record consistency and evidence bytes; reviewers still assess whether the evidence actually supports its claim. Its `ready` output concerns this access/readiness register only. Confirmation validity and required reconfirmation govern access readiness; later product-test results and the complete release remain governed by the programme and release gates. Run the checker against a frozen trusted repository: these portable file checks are not a sandbox against concurrent hostile filesystem changes.

```sh
python3 scripts/check_release_readiness.py --as-of 2026-09-24
python3 scripts/check_release_readiness.py --require-ready
python3 -m unittest discover -s scripts/tests -p 'test_release_readiness.py' -v
```

The consistency command passes for an honest incomplete record. `--require-ready` deliberately exits nonzero while any requirement is unresolved or its confirmation is stale, including after programme end. Use the current local date by omitting `--as-of`; an explicit date reproduces a historical assessment. The existing source CI discovers the isolated test module automatically. None of the synthetic tests confirms actual credentials, hardware access or reviewer commitments.

EW-08 can close only when its required availability, role assignments and OS decisions are confirmed. Any owner-approved scope/date change must be recorded separately; it does not itself confirm access. Creating this checklist alone does not close the issue or weaken a release gate.
