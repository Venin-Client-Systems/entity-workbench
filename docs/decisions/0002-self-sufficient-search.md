# ADR 0002 — direct collection and a local web corpus

Status: accepted user clarification, 2026-09-22.

The user prohibited external search providers and explicitly allowed direct public website collection with local indexing. Remove provider selection and all account/API-key requirements. There is no external search API adapter in the application.

The analyst supplies HTTPS seed URLs and previews disclosure. The Rust broker collects public pages on those selected hosts, honours robots rules, verifies each destination, pins resolved public IP addresses and validates every redirect. Only static text and discovered same-host links are extracted; source scripts are never executed.

The embedded Lucene engine searches imported and collected sources without network access. Source coverage and index revision are visible. An installation with no collected corpus has no open-web search results. Broad current web coverage is an unresolved acceptance requirement, not an automatic consequence of owning a crawler.

No hosted metasearch instance, browser handoff, CAPTCHA workaround or mock provider result substitutes for this design.
