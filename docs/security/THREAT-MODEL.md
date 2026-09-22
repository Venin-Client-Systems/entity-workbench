# Threat model and residual risk

Protect canonical observations, original evidence, local addresses/transactions, analysis integrity and bounded network disclosure against hostile documents, pages, worker outputs and accidental analyst mistakes.

| Boundary | Current mechanism | Residual risk / required gate |
|---|---|---|
| Untrusted text → UI | React/text rendering; CSP forbids source scripts/frames; report escapes content | More format/DOM fuzzing; production webview coverage |
| UI → Rust | Tagged strict commands, validation, fixed SQL, expected revision on analyst decisions | General schema fuzzing, IPC payload/resource bounds |
| Worker → workspace | No worker database path; versioned messages; selected output/IDs/revision validated | Native Windows launcher absent; macOS signed helpers absent |
| macOS search worker | Deny-by-default Seatbelt profile; job/cache writes only; runtime/system reads; no network; 256 MiB Java heap and timeout | Uses private dyld bootstrap profile; global metadata reads; native memory/disk/descendant-process limits not fully proven |
| Hostile parsing | Parser adapter remains disabled in desktop | PDF/Office/archive/OCR compatibility and resource-exhaustion validation required |
| Web destinations | HTTPS only, explicit hosts, all resolved IPs checked, DNS pinned, no proxies/cookies/referrer, manual redirects, capped response | DNS helper threads can outlive timeout; formal rebinding/redirect harness and cancellation required |
| Source truth | Original checksums, reversible corrections, separate review states and report snapshots | Local administrator/same-user tampering is outside application encryption claims |
| Backups | SQLite consistent backup plus every referenced original; hash checks; restore refuses overwrite/newer schema | Future migration rollback and crash injection; backup manifest authentication not implemented |
| Statistical identity | No automatic merge; statistical linkage disabled without calibrated model | Calibration, bias/error evaluation, comparison UI and independent-source modelling |

A restrictive development probe initially prevented Java startup. Importing the installed OS dyld bootstrap profile repaired JVM compatibility in the tested environment. The resulting hostile probe denied cross-workspace file content, original modification and direct network connection while permitting job I/O. This is bounded evidence for that profile and machine, not proof for other OS versions or all native libraries.

Worker isolation must also prevent leakage through child processes, IPC, inherited file descriptors and platform services. The current probes do not exhaustively prove those paths. Release remains blocked until platform-specific tests and signed packaging validate them.

No application background updater, telemetry service, automatic crash upload or credential store is used. Website TLS checks are never disabled. Robots failures are treated conservatively; collection success does not grant reuse or redistribution rights.
