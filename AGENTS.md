# Entity Workbench development rules

- Preserve the Rust domain layer as the only canonical writer. Workers are engine adapters.
- Use synthetic fixtures only. Never commit real investigation data, private paths, credentials, employer attribution or deployment configuration.
- Keep the private exclusion list outside the repository; run `scripts/audit_public.py` on the staged files before publication.
- No external search providers, accounts, API keys or separately operated server. Direct analyst-selected public web collection and local indexing are in scope.
- Never execute collected HTML/SVG/scripts in the privileged interface. Use escaped text and validated derivatives.
- Do not describe process separation as sandboxing. Unsupported confinement must fail closed.
- Keep `docs/STATUS.md`, `docs/VERIFICATION.md` and `docs/release-gates.json` accurate. A partial build must not be labelled an approved release.
- Run relevant Rust tests and strict Clippy, UI build/workflow tests, and engine tests for affected adapters. Cross-platform release gates require actual platform evidence.
- No schema migration without a consistent recoverable backup and restore test covering referenced evidence.
- Original code is Apache-2.0; preserve third-party licences and notices.
- Use an editable product-design tool for visual design. Keep design frame links, component specifications and rendered comparison evidence in `docs/design/`. The current interface is a prototype until that pass is completed; do not imply screenshots alone are design validation.
