"""Closed fixed Search campaign receipt. Echoed identities do not prove execution."""
import hashlib
import json
import math
import re

POLICY = "fixed-coordinator-search-five-recipes-v1"
PREFIX = "EW_NATIVE_SEARCH_EVENT="
TEST = "coordinator::search::native::native_coordinator_search_campaign"
RECIPES = ["index", "search", "search", "search", "search"]
QUERIES = ["cooperative", "Mira AND depot", "(", "riverside"]
TEXTS = [("alpha.txt", b"Rowan Ellis synthetic cooperative alpha memorandum.\n"),
         ("beta.txt", b"Rowan Ellis synthetic riverside beta memorandum.\n"),
         ("gamma.txt", b"Mira Chen synthetic gamma depot note.\n")]
FIXTURES = [{"name": name, "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}
            for name, data in TEXTS]
ORIGINALS = [dict(item, anchor={"line_start": 1, "line_end": 1}) for item in FIXTURES]


class InvalidReceipt(ValueError):
    pass


class EventDecodeFailure(InvalidReceipt):
    def __init__(self, reason, events):
        super().__init__(reason)
        self.events = events


def require(ok, reason):
    if not ok:
        raise InvalidReceipt(reason)


def unique(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate_receipt_key")
        result[key] = value
    return result


def decode(text):
    events = []
    for line in text.splitlines():
        if PREFIX not in line:
            continue
        try:
            require(len(events) < 15 and len(line.encode()) <= 256 * 1024, "event_bound_exceeded")
            raw = line.split(PREFIX, 1)[1]
            event = json.loads(raw, object_pairs_hook=unique, parse_constant=lambda _: (_ for _ in ()).throw(InvalidReceipt("nonfinite_json")))
            require(type(event) is dict, "event_not_object")
            events.append(event)
        except (InvalidReceipt, ValueError, TypeError) as error:
            raise EventDecodeFailure("malformed_event_tail", events) from error
    return events


def keys(value, fields):
    require(type(value) is dict and set(value) == set(fields.split()), "unexpected_receipt_fields")


def same(a, b):
    # Python's True == 1 must never turn a mistyped observation into acceptance.
    return json.dumps(a, sort_keys=True, separators=(",", ":")) == json.dumps(b, sort_keys=True, separators=(",", ":"))


def sha(value):
    return type(value) is str and re.fullmatch(r"[0-9a-f]{64}", value) is not None


def integer(value, minimum=0):
    return type(value) is int and value >= minimum


def index(value, revision):
    keys(value, "files marker logical_bytes")
    require(type(value["files"]) is list and 1 <= len(value["files"]) <= 128, "index_member_count")
    names, total = [], 0
    for row in value["files"]:
        require(type(row) is list and len(row) == 5, "index_row_shape")
        name, dev, ino, count, digest = row
        require(type(name) is str and re.fullmatch(r"[A-Za-z0-9._\-]{1,180}", name)
                and name not in (".", "..") and integer(dev) and integer(ino, 1)
                and integer(count) and count <= 8 * 1024 * 1024 and sha(digest), "index_row_invalid")
        names.append(name)
        total += count
    require(names == sorted(set(names)) and total <= 24 * 1024 * 1024
            and integer(value["logical_bytes"]) and value["logical_bytes"] == total, "index_aggregate")
    marker = value["marker"]
    require(type(marker) is list and len(marker) == 4 and integer(marker[0]) and integer(marker[1], 1)
            and type(marker[2]) is int and marker[2] == len(str(revision))
            and marker[3] == hashlib.sha256(str(revision).encode()).hexdigest(), "index_revision_marker")


def validate(events, source, nonce, runtime, exit_code):
    require(type(exit_code) is int and exit_code == 0, "native_exit_failed")
    order = ["start", "runtime_verified", "healthy_start"] + [kind for _ in QUERIES for kind in ("query_before", "query_after")] + ["healthy_complete", "intent_complete", "final"]
    require(len(events) == len(order), "incomplete_native_events")
    for event, kind in zip(events, order):
        keys(event, "policy source nonce runtime_sha256 kind details")
        require(event["policy"] == POLICY and event["source"] == source and event["nonce"] == nonce
                and event["runtime_sha256"] == runtime["sha256"] and event["kind"] == kind,
                "native_event_identity_or_order")
    require(same(events[0]["details"], {"recipe_entry_limit": 5, "worker_seconds": 30, "queries": QUERIES, "fixtures": FIXTURES}), "start_contract")
    require(same(events[1]["details"], {"sha256": runtime["sha256"], "files": runtime["files"]}), "native_runtime_inventory")
    start = events[2]["details"]
    keys(start, "workspace_revision canonical_sha256 originals")
    revision = start["workspace_revision"]
    require(integer(revision, 1) and sha(start["canonical_sha256"]) and same(start["originals"], ORIGINALS), "canonical_start")
    afters, first_index = [], None
    for i, query in enumerate(QUERIES):
        before, after = (events[3 + i * 2 + offset]["details"] for offset in (0, 1))
        require(same(before, {"sequence": i, "query": query, "recipe_entries": RECIPES[:0 if i == 0 else i + 1]}), "query_before_contract")
        keys(after, "sequence query result recipe_entries index canonical_unchanged originals_unchanged assignment_cleanup")
        require(type(after["sequence"]) is int and after["sequence"] == i and after["query"] == query
                and after["recipe_entries"] == RECIPES[:i+2]
                and all(after[name] is True for name in ("canonical_unchanged", "originals_unchanged", "assignment_cleanup")), "query_after_contract")
        if i == 2:
            require(after["result"] == {"error": "validation"}, "malformed_query_not_known_failure")
        else:
            result = after["result"]
            keys(result, "workspace_revision hits total")
            require(result["workspace_revision"] == str(revision) and type(result["total"]) is int and result["total"] == 1
                    and type(result["hits"]) is list and len(result["hits"]) == 1, "fixed_query_result")
            hit = result["hits"][0]
            keys(hit, "id name score")
            expected = FIXTURES[{0: 0, 1: 2, 3: 1}[i]]
            require(hit["id"] == expected["sha256"] and hit["name"] == expected["name"]
                    and type(hit["score"]) in (int, float) and math.isfinite(hit["score"]) and hit["score"] > 0, "fixed_hit_identity")
        index(after["index"], revision)
        if first_index is None:
            first_index = after["index"]
        require(after["index"] == first_index, "index_rebuilt_or_changed")
        afters.append(after)
    healthy = events[-3]["details"]
    require(same(healthy, dict(start, queries=afters, recipe_entries=RECIPES, shutdown_joined=True, ownership_released=True)), "healthy_completion")
    refusal = events[-2]["details"]
    keys(refusal, "host_injected unknown_child_created search_error recipe_entries canonical_unchanged canonical_sha256 workspace_revision originals intent_identity index_sentinel_identity intent_unchanged index_unchanged shutdown_joined shutdown_quarantined ownership_released")
    require(all(refusal[name] is True for name in ("host_injected", "canonical_unchanged", "intent_unchanged", "index_unchanged", "shutdown_joined", "shutdown_quarantined"))
            and refusal["unknown_child_created"] is False and refusal["ownership_released"] is False
            and refusal["search_error"] == "blocked" and refusal["recipe_entries"] == []
            and integer(refusal["workspace_revision"], 1) and sha(refusal["canonical_sha256"])
            and same(refusal["originals"], ORIGINALS), "retained_intent_control")
    for name, data in (("intent_identity", b"host-injected retained execution intent; no unknown process created by this control\n"),
                       ("index_sentinel_identity", b"unmodified synthetic derivative")):
        row = refusal[name]
        require(type(row) is list and len(row) == 4 and integer(row[0]) and integer(row[1], 1)
                and type(row[2]) is int and row[2] == len(data)
                and row[3] == hashlib.sha256(data).hexdigest(), "refusal_retained_identity")
    require(same(events[-1]["details"], {"passed": True, "healthy": healthy, "retained_intent": refusal,
            "runtime_unchanged": True, "runtime_sha256": runtime["sha256"], "recipe_entries": RECIPES,
            "native_scope": "macos_selected_development_runtime"}), "final_binding")
    return {"healthy_ownership_released": True, "inert_intent_refused_before_recipe": True,
            "recipe_entry_count": 5, "recipe_entries_are_spawn_evidence": False}
