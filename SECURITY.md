# Security review entry point

**Development software. Complete security and distribution approval has not been established. Use synthetic evidence for evaluation.**

Start with the [process/data-flow diagram](docs/ARCHITECTURE.md), [threat model](docs/security/THREAT-MODEL.md), [process permissions](docs/security/PERMISSIONS.md), [connector manifest](docs/security/connectors.json), [verification record](docs/VERIFICATION.md) and [machine-readable release gates](docs/release-gates.json).

Dependency lockfiles are `Cargo.lock`, `package-lock.json`, and `workers/python/uv.lock`. Java versions are pinned in `workers/java/pom.xml`; the generated Maven SBOM records transitive dependencies. Public dependency inventories are in `sbom/`. [Operations guidance](docs/OPERATIONS.md) covers data lifecycle, recovery and updates.

Do not put real evidence, personal information, credentials or exploit targets in public issues. A private vulnerability-reporting channel has not yet been established for a supported release. Public reports should contain only sanitized descriptions and synthetic reproductions.

No application-level workspace encryption is claimed. Use appropriate OS/storage encryption and access controls for all originals, databases, indexes, caches, backups and exports. This recommendation does not turn the current development build into an approved release.
