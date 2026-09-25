"""Offline validator for the fixed native durable HTTPS campaign; no network I/O."""
import ipaddress
import hashlib
import json
import re
import uuid
from urllib.parse import urlsplit

from test_native_collection_transport import Failure

POLICY = "fixed-owned-synthetic-https-two-attempts-v1"
PREFIX = "EW_NATIVE_HTTPS_EVENT="
SEED = "https://raw.githubusercontent.com/Venin-Client-Systems/entity-workbench/df4dbff063d274effb9fa6385095938751bb300e/fixtures/brief.txt"
ROBOTS = "https://raw.githubusercontent.com/robots.txt"
FIXTURE_SHA = "1b728541f9939c78bd3432482c1b5894ff70ffc017ebbe07a2f4b0535ba99371"
FIXTURE_BYTES = 863
ORDER = ["start", "queued", "launch", "observation", "launch", "observation", "final"]


def need(condition, reason):
    if not condition:
        raise Failure(reason)


def digest(value):
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None


def exact(actual, expected):
    return json.dumps(actual, sort_keys=True) == json.dumps(expected, sort_keys=True)


def preview_digest(preview):
    # Reconstruct the closed Rust PreviewPayload/CollectionInput field order.
    payload = {"schema_version": preview["schema_version"], "collector_policy": preview["collector_policy"],
               "input": {key: preview["input"][key] for key in ("urls", "max_hops", "max_requests", "max_seconds")},
               "selected_hosts": preview["selected_hosts"], "robots_urls": preview["robots_urls"],
               "disclosure": {key: preview["disclosure"][key] for key in ("dns_hostnames", "connection_metadata",
               "selected_and_followed_urls", "automatic_case_contents", "followed_hosts")}}
    return hashlib.sha256(json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode()).hexdigest()


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        need(key not in value, "duplicate_event_key")
        value[key] = item
    return value


class EventDecodeFailure(Failure):
    def __init__(self, reason, events):
        super().__init__(reason)
        self.events = events


def decode(output):
    events = []
    for line in output.splitlines():
        if PREFIX in line:
            try:
                event = json.loads(line.split(PREFIX, 1)[1], object_pairs_hook=unique_object)
                need(isinstance(event, dict), "nonobject_event")
            except (Failure, ValueError) as error:
                reason = str(error) if isinstance(error, Failure) else "malformed_event_json"
                raise EventDecodeFailure(reason, events) from None
            events.append(event)
    return events


def validate(events, source, nonce, exit_code):
    need(re.fullmatch(r"[0-9a-f]{40}", source) is not None
         and str(uuid.UUID(nonce)) == nonce, "invalid_source_or_nonce")
    need(type(exit_code) is int and exit_code == 0 and [e.get("kind") for e in events] == ORDER,
         "native_failure_or_incomplete_campaign")
    for event in events:
        need(event.get("policy") == POLICY and event.get("source") == source
             and event.get("nonce") == nonce, "source_nonce_or_policy_mismatch")
    start, queued, _, _, _, _, final = [e["details"] for e in events]
    need(exact(start, {"fixture_sha256": FIXTURE_SHA, "fixture_bytes": FIXTURE_BYTES,
                   "max_requests": 2, "max_hops": 0, "max_seconds": 20, "synthetic": False}),
         "fixed_scope_changed")
    scope = {"urls": [SEED], "max_hops": 0, "max_requests": 2, "max_seconds": 20}
    preview = queued["preview"]
    need(preview["schema_version"] == 1 and exact(preview["input"], scope)
         and preview["collector_policy"] == "direct-https-durable-v3"
         and preview["selected_hosts"] == ["raw.githubusercontent.com"]
         and preview["robots_urls"] == [ROBOTS] and digest(preview["preview_sha256"])
         and preview["preview_sha256"] == preview_digest(preview),
         "preview_scope_or_identity_mismatch")
    need(exact(preview["disclosure"], {"dns_hostnames": True, "connection_metadata": True,
                                  "selected_and_followed_urls": True,
                                  "automatic_case_contents": False,
                                  "followed_hosts": "selected_hosts_only"}), "disclosure_changed")
    need(queued["request_key"] == nonce and queued["record_version"] == 3
         and queued["synthetic"] is False and queued["production_native_enabled"] is False,
         "queue_identity_or_mode_mismatch")
    for key in ("passed", "shutdown_ok", "ownership_released", "reopened_verified",
                "backup_restored_verified", "observations_unchanged", "receipt_settled_exactly",
                "acquisition_valid"):
        need(final.get(key) is True, "incomplete_owned_operation_or_recovery")
    need(final["synthetic"] is False and final["production_native_enabled"] is False
         and exact(final["guard"], {"admitted": 2, "refused": False})
         and final["fixture_sha256"] == FIXTURE_SHA and final["fixture_bytes"] == FIXTURE_BYTES
         and digest(final["canonical_record_sha256"]), "invalid_final_identity_or_guard")
    need(type(final["elapsed_milliseconds"]) is int and 0 <= final["elapsed_milliseconds"] < 30000,
         "owned_campaign_time_missing_or_exceeded")
    inspection = final["inspection"]
    run = inspection["run"]
    need(type(inspection["workspace_revision"]) is int and inspection["workspace_revision"] > 0
         and inspection["availability"] == "standalone_unavailable"
         and exact(inspection["controls"], {"can_cancel": False, "can_resume": False, "can_retry_settlement": False}),
         "public_controls_or_revision_invalid")
    need(inspection["schema_version"] == 1 and inspection["native_execution_enabled"] is False
         and run["id"] == queued["job_id"] and run["request_key"] == nonce
         and run["record_version"] == 3 and run["mode"] == "live"
         and run["collector_policy"] == "direct-https-durable-v3" and exact(run["input"], scope)
         and run["state"] == "successful" and run["generation"] == 1
         and run["requests_used"] == 2 and run["pages_retained"] == 1
         and run["cancellation_requested"] is False, "canonical_run_mismatch")
    need(type(run["first_started_at_ms"]) is int
         and run["deadline_at_ms"] == run["first_started_at_ms"] + 20000,
         "first_deadline_not_preserved")
    for value in (preview["schema_version"], queued["record_version"], inspection["schema_version"],
                  run["record_version"], run["generation"], run["requests_used"], run["pages_retained"],
                  run["deadline_at_ms"]):
        need(type(value) is int, "noninteger_canonical_count_or_version")
    requests = inspection["requests"]
    need(len(requests) == 2, "wrong_canonical_request_count")
    for index, request in enumerate(requests):
        launch = events[2 + 2 * index]["details"]
        observed = events[3 + 2 * index]["details"]
        url = [ROBOTS, SEED][index]
        need(exact(launch, {"sequence": index, "url": url, "job_id": run["id"], "generation": 1}),
             "unapproved_launch")
        need(request["sequence"] == index and request["generation"] == 1
             and exact(request["entry"], {"url": url, "hop": 0, "redirects": 0,
                                      "purpose": ["robots", "seed"][index], "parent": None}),
             "canonical_request_ancestry_changed")
        need(request["progress"]["state"] == "observed" and observed["sequence"] == index
             and request["progress"]["receipt"] == observed["receipt"], "settlement_changed_observation")
        receipt = observed["receipt"]
        for value in (request["sequence"], request["generation"], request["reserved_at_ms"],
                      receipt["schema_version"], receipt["observed_wall_ms"]):
            need(type(value) is int, "noninteger_request_identity_or_clock")
        need(receipt["schema_version"] == 1 and receipt["phase"] == "body"
             and receipt["http_delivery"] == "may_have_been_sent"
             and receipt["locally_quiescent"] is True and receipt["stop_observed"] is None
             and receipt["resolver_uncertainty"] is None, "incomplete_or_uncertain_transport")
        need(type(receipt["elapsed_milliseconds"]) is int and 0 <= receipt["elapsed_milliseconds"] <= 20000
             and request["reserved_at_ms"] <= receipt["observed_wall_ms"] < run["deadline_at_ms"],
             "invalid_or_expired_clock_observation")
        candidates = receipt["resolved"]
        need(candidates["method"] == "macos_dns_service_observed_batch"
             and candidates["authoritative_complete_set"] is False
             and 1 <= len(candidates["addresses"]) <= 64, "native_candidate_snapshot_missing")
        for socket in candidates["addresses"]:
            address = urlsplit("//" + socket)
            need(address.port == 443 and ipaddress.ip_address(address.hostname).is_global,
                 "unsafe_observed_candidate")
        outcome = receipt["outcome"]
        need(outcome["kind"] == "complete" and digest(outcome["sha256"])
             and type(outcome["bytes"]) is int and 0 <= outcome["bytes"] <= 2 * 1024 * 1024,
             "complete_body_identity_missing")
        head = outcome["head"]
        need(head["identity_encoding"] is True and head["redirect_url"] is None
             and head["status"] in ([200, 404] if index == 0 else [200]), "refused_or_redirected_response")
        need(request["original"] == {"evidence_id": outcome["sha256"], "sha256": outcome["sha256"],
                                     "bytes": outcome["bytes"]}, "retained_original_mismatch")
        if index == 1:
            need(outcome["sha256"] == FIXTURE_SHA and outcome["bytes"] == FIXTURE_BYTES,
                 "published_synthetic_fixture_changed")
