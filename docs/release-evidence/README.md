# Release evidence and acceptance policy

The release ledger is [`../release-gates.json`](../release-gates.json), version 2. It retains all twelve gates, an optional release candidate and versioned observations. The repository currently has **no candidate, no acceptance observations and no passed gates**. Source tests exercise invented candidates in temporary directories; their successful outcomes are not product-release evidence.

[`../../packaging/release-acceptance.v1.json`](../../packaging/release-acceptance.v1.json) maps every gate to delivery issues, required platforms, evidence kind, versioned procedure and reviewer role. It also maps the ten end-to-end scenarios. [`../../fixtures/acceptance/catalogue.v1.json`](../../fixtures/acceptance/catalogue.v1.json) binds the initial synthetic inputs to expected assertions and scenario procedures by file size and SHA-256. These procedures still require implementation-specific executable harnesses and actual installed-artifact runs.

## Two distinct checks

Run the structural, integrity and policy check in source CI:

```sh
python3 scripts/release_gate.py --check-policy
python3 -m unittest discover -s scripts/tests -p 'test_*.py' -v
```

Policy checking verifies retained evidence and review-receipt bytes, but does not inspect candidate installer bytes. It always returns `complete_release: false` and `candidate_bytes_verified: false`, including for a consistent ledger that claims complete acceptance. Exit 0 here means the declarations and retained evidence agree, not that a product is released.

For final local verification, stage immutable evidence and the actual downloadable artifacts, then run:

```sh
python3 scripts/release_gate.py \
  --ledger docs/release-gates.json \
  --evidence-root artifacts/release-evidence \
  --artifact-root artifacts/release-candidate
```

This mode returns exit 0 only when all gate/platform/required-OS observations qualify, the ledger claims agree, and all three artifact files match their sizes and hashes. Current repository defaults return exit 1 because the release remains unpassed. Neither mode fetches URLs, runs procedure strings, invokes signers or executes attached evidence.

## Candidate and observation bindings

Public structural contracts are [`release-ledger.v2.schema.json`](../../schemas/release-ledger.v2.schema.json), [`release-evidence.v1.schema.json`](../../schemas/release-evidence.v1.schema.json) and [`release-review.v1.schema.json`](../../schemas/release-review.v1.schema.json). The Python verifier adds contextual constraints that JSON Schema cannot establish.

A candidate identifies one source Git revision, the exact acceptance-policy and fixture-catalogue hashes, and distinct artifact references for Windows x64, macOS Apple Silicon and macOS Intel. Rebuilding an installer changes its acceptance identity even when its source revision is unchanged. Changing policy or fixture bytes requires new bindings and fresh reviewed observations.

Each observation records its gate, platform, source/artifact/policy/catalogue hashes, UTC observation time, normalized OS version, architecture, memory, environment context, procedure ID/version/invocation, outcome, attachments and review. The invocation is a descriptive string. Installed-artifact evidence is required; `source_check`, `hosted_ci` and `development` observations cannot qualify. Clean-install and downloaded-artifact gates require `clean_install`, and the benchmark requires 16 GiB.

The support matrix is intentionally unresolved: minimum OS versions and required test versions need a recorded support decision and actual environment access. Numeric OS triples have no leading zeros. Windows uses marketing version, minor version and build (for example `11.0.26100`); macOS uses major, minor and patch. Do not copy a kernel version into the Windows marketing-version field. A confirmed matrix must include its minimum version and every additional version promised for acceptance testing. Every gate must have qualifying evidence at every required version.

The latest observation governs separately for each gate, target and OS version. A failed, blocked, not-run, pending or rejected latest observation blocks that combination; rejecting a review does not erase an observed failure. A later accepted pass at that same version is necessary to supersede it. A pass on another OS cannot hide the failure. Additional supported OS versions observed for unchanged source/artifact bytes also require current qualifying evidence, even when their older observation used an earlier policy or catalogue. Noncurrent records remain retained and are reported.

The initial engineering policy allows observations up to **7 days** old for live discovery, dependency/security review, signatures and downloadable-artifact verification; other gates allow **30 days**. These are proposed release-process defaults for maintainer review, not an owner-confirmed service guarantee. Future dates are invalid. Change the versioned policy through review if different freshness is justified; the new policy hash invalidates older acceptance bindings.

## Retained review receipts

A pending review has null role, reference and receipt. An accepted or rejected review identifies a local receipt by safe relative path, byte size and SHA-256. The receipt records the evidence ID, review reference, decision, role, UTC review time, reason and `observation_sha256`.

Compute the observation digest from the entire observation excluding only its `review` member, serialized as Python `json.dumps(payload, sort_keys=True, separators=(',', ':'), ensure_ascii=True)` and encoded as UTF-8. This binds the review to the source, artifact, procedure, outcome and attachment hashes. The receipt must match the observation and decision, and may not predate it. Security gates require the security-reviewer role; other gates require release-verifier. A generic maintainer receipt is retained but does not satisfy either role requirement.

Receipts are **reviewed declarations**, not authenticated identities or cryptographic signatures. This offline tool checks consistency and integrity; it cannot prove that a test ran, an attachment is truthful, a declared reviewer is independent, or a signature/trust service approved an installer. Trusted evidence production, reviewer identity checks, substantive inspection and required repository review remain release responsibilities. Do not treat an arbitrary self-authored JSON pass as acceptance.

## Storage and resource limits

Stage a frozen local tree while checking. Paths are relative, portable and noncolliding; traversal, links, reparse points and hardlinked attachment/artifact files are rejected. Hashing checks file identity and changes during reading. This is a build/review tool, not an adversarial filesystem sandbox; do not let another process mutate its roots during verification.

The ledger allows 2,000 records, at most 20 attachments per record, 64 MiB per attachment/receipt and 1 GiB total declared evidence references. Candidate installers are bounded at 32 GiB each. JSON input uses the shared bounded strict reader, rejecting duplicate keys and non-finite numbers. Larger media must be summarized into reviewed bounded evidence or motivate a separately reviewed policy change. Preserve originals under the evidence-retention policy.

Retain negative attempts and prior records in version history and the evidence store. The verifier cannot detect a deliberately deleted historical record, so changes removing observations require explicit review. Keep sensitive originals and identities outside the public repository; public receipts must be sanitized and supported by permitted retained evidence. All repository acceptance fixtures are fictional.
