# Owner availability update — 24 September 2026

The project owner explicitly confirmed that they are the sole human contributor and have only the current Mac available. This record is a sanitized summary of that owner statement, not a new account, hardware or credential inspection.

The current development machine was previously observed to be an Apple Silicon Mac with 16 GiB of physical memory; see the retained [initial audit](audit-2026-09-24.md). The owner statement establishes current resource constraints:

- No Windows 11 x64 test environment is available.
- No Intel Mac test environment is available.
- No additional human maintainer or security reviewer is assigned or available for the required independent review.
- The existing Apple Silicon development Mac is available for development. A clean test environment and a reserved, reproducible benchmark arrangement on it remain unverified.

The first three constraints are explicit unavailability evidence for the Windows, Intel Mac, maintainer-review and security-review readiness requirements. Those requirements are recorded as `missing`, not merely unknown. Existing hosted source-build checks do not provide the missing clean installed-artifact environments or independent human release review.

Signing accounts, usable signing routes, Developer ID/notarization access and specific signing/release role acceptance remain unknown. The statement does not assign those responsibilities to the owner, establish minimum supported OS versions, reserve the Mac, authorize resetting it, or remove any review requirement.

EW-08 remains open. Additional access and independent review, or explicit changes to the product scope or schedule, require further decisions and evidence. No release gate or independence requirement is reduced by this record. Preserve it alongside subsequent availability updates so the original constraint remains visible.
