# Pure selected-publisher profile admission

`collection_profile.rs` defines `selected-publisher-access-v1` as pure local admission data. This is not a durable record, public command, network broker, access decision or live benchmark. Existing v4 policies, input types, machine/replay, transport and schemas are unchanged; normal native collection remains disabled.

This admission type is benchmark-specific, not the general production collection plan. Embedding the frozen public metadata grants no access permission and does not implicitly select any website for execution.

## Exact frozen binding

The only catalogue is the compiled `public-benchmark.v1.json`, pinned to SHA-256 `9fc07bacc9fa73dbccd6352e3cd2f938a46b7f2b4a3f3ed51726dfb0f83840f6`. Admission checks both the supplied hash and actual embedded bytes. It then checks the exact task-to-publisher relationship, publisher domain, frozen seed and SHA-256 of the exact frozen query's UTF-8 bytes. Case, whitespace or seed changes cannot be silently normalized into another task. The local query text is never a plan input or part of the URL disclosure.

The plan also binds a canonical lowercase, non-nil session UUID, ordered explicit access URLs, and effective request/time/hop limits. A UUID only identifies the caller's declared session; validation does not prove an empty corpus or create/own a workspace. Those remain future store/coordinator responsibilities.

`PublisherAccessInput` is untrusted local data. `ValidatedPublisherAccessPlan` has private fields, no deserializer or mutable accessor, and is constructed only by validation. Its digest is SHA-256 of compact serde JSON with `profile` followed by `input`; input fields use their declared struct order, including access URL order and all three limit fields. It can identify a later reviewed plan, but cannot authorize execution or establish reviewer approval.

## URL and limit rules

Each URL is at most 2,048 bytes and must already be canonical ASCII URL serialization: HTTPS, a DNS hostname, implicit port 443, no userinfo, query or fragment. Explicit `:443` is refused as a noncanonical alias, matching the frozen scorer's plain-authority rule. Percent-encoded paths are allowed; controls, whitespace, backslashes, parser-normalized aliases, IP literals and malformed DNS labels are refused. The shared v4 URL normalizer is not modified.

Hosts must equal the exact frozen publisher domain or have its dot-separated suffix. This does not infer a registrable domain, follow a redirect, resolve DNS or validate an address. In particular, CERN's frozen domain is `home.cern`; `assets.home.cern` is in syntax scope, while `atlas.cern` and `www.cern` are not. Scope matching does not prove publisher control of every subdomain.

There can be zero to 50 distinct selected access URLs. Zero records no selection; it does not imply terms permission. `selected_access_url` checks exact membership as well as syntax/scope. It does not infer whether a URL contains terms, robots, ordinary content or an answer. Caller-selected paths remain disclosures. `validate_destination_syntax` only checks the broader publisher URL syntax; it provides no content lineage, request purpose, DNS safety, reserved charge or access approval.

Limits are 0–2 hops, 0–50 request attempts and 1–600 seconds. A zero-request plan is valid inert metadata and cannot be treated as permission to make an access request. The pure `tighten` operation can only reduce or retain each previously admitted limit and recomputes the plan digest. It does not start a clock, reserve/refund a request, pause a deadline or promise that every selected access URL will fit the budget. Access/robots, content, redirects and failures must eventually share these same ceilings.

## Explicit remaining integration and policy limits

No robots/terms acquisition or review state is implemented here. A future versioned record and coordinator integration must retain charged access receipts, bind a reviewer decision to exact originals, preserve the first deadline across human review/recovery, exclude access material from indexing/content ancestry, and refuse execution when ownership or transport quiescence is unknown. It must establish the cold corpus before any access acquisition and prohibit preload or foreign index reuse. This module creates none of that authority or evidence.

The frozen README's initial-request wording and its requirement for prior budgeted access review still require a recorded benchmark design clarification. Treating the seed as the first **content** request remains a proposal, not an implemented change to frozen policy or campaign eligibility. Tasks, criteria, thresholds, scorer and frozen bytes are untouched. Independent labels remain absent until actual independent review; syntax admission provides no relevance credit or release acceptance.

Tests use the frozen public metadata and fabricated URL/session examples without fetching any source. They cover all ten publisher groups, name/domain/organisation query bindings, suffix lookalikes, malformed plans and URL aliases, selection caps, lower quotas, deterministic binding, disclosure exclusion and unchanged historical v4 behavior. No native/network campaign is part of this slice.
