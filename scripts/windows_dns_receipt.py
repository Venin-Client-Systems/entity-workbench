"""Closed validation of fixed Windows native DNS observations, not DNS evidence itself."""
import json
import re
import uuid

POLICY = "fixed-three-windows-lookups-no-http-v1"
PREFIX = "EW_WINDOWS_DNS_CASE="
CASES = ["transport_pre_cancel", "transport_expired", "native_success",
         "native_negative", "native_pending_cancel"]
HOSTS = ["example.com", "example.com", "example.com",
         "ew-native-proof.invalid", "ew-native-cancel-proof.invalid"]
PENDING = {996, 997, 10036}
PROBE_FIELDS = {"wsa_startup", "events_created", "contexts_created", "launch_returns",
                "cancel_returns", "completion_returns", "wait_signalled", "wait_timeouts",
                "wait_errors", "contexts_dropped", "contexts_retained", "results_freed",
                "event_close", "wsa_cleanup", "trace_truncated"}


def require(value):
    if not value:
        raise ValueError("closed native DNS receipt rejected")


def pairs(items):
    value = {}
    for key, item in items:
        require(key not in value)
        value[key] = item
    return value


def decode(output):
    require(type(output) is str and len(output.encode("utf-8")) <= 1024 * 1024)
    events = []
    for line in output.splitlines():
        if PREFIX in line:
            require(len(events) < 5)
            events.append(json.loads(line.split(PREFIX, 1)[1], object_pairs_hook=pairs,
                                     parse_constant=lambda _: require(False)))
    return events


def probe(value):
    require(type(value) is dict and set(value) == PROBE_FIELDS)
    require(type(value["trace_truncated"]) is bool)
    for key in ("events_created", "contexts_created", "contexts_dropped", "contexts_retained", "results_freed"):
        require(type(value[key]) is int and 0 <= value[key] <= 1)
    for key in ("wait_signalled", "wait_timeouts"):
        require(type(value[key]) is int and 0 <= value[key] <= 100_000)
    for key in ("wsa_startup", "launch_returns", "cancel_returns", "completion_returns", "wait_errors", "wsa_cleanup"):
        require(type(value[key]) is list and len(value[key]) <= 64)
        require(all(type(item) is int and -(2**31) <= item <= 2**32-1 for item in value[key]))
    require(type(value["event_close"]) is list and len(value["event_close"]) <= 1
            and all(type(item) is bool for item in value["event_close"]))
    require(len(value["launch_returns"]) <= 1 and len(value["wsa_startup"]) <= 1
            and len(value["wsa_cleanup"]) <= 1 and len(value["cancel_returns"]) <= 2)
    if value["launch_returns"] != [997]:
        require(not value["cancel_returns"] and not value["completion_returns"]
                and not value["wait_signalled"] and not value["wait_timeouts"] and not value["wait_errors"])
    require(value["wait_signalled"] >= len(value["completion_returns"]))
    if value["contexts_dropped"]:
        # A signalled event or cancel return alone cannot authorize release.
        require(not value["contexts_retained"])
        if value["launch_returns"] == [997]:
            require(value["completion_returns"] and value["completion_returns"][-1] not in PENDING)
    if value["contexts_retained"]:
        require(value["contexts_created"] == 1 and not value["contexts_dropped"]
                and not value["event_close"] and not value["wsa_cleanup"] and not value["results_freed"])
    return value


def zero(value):
    return (all(value[key] == 0 for key in ("events_created", "contexts_created", "contexts_dropped",
                                           "contexts_retained", "results_freed", "wait_signalled", "wait_timeouts"))
            and all(value[key] == [] for key in ("wsa_startup", "launch_returns", "cancel_returns",
                                               "completion_returns", "wait_errors", "event_close", "wsa_cleanup"))
            and value["trace_truncated"] is False)


def released(value):
    return (value["wsa_startup"] == [0] and value["events_created"] == 1
            and value["contexts_created"] == 1 and value["contexts_dropped"] == 1
            and value["contexts_retained"] == 0 and value["event_close"] == [True]
            and value["wsa_cleanup"] == [0] and value["trace_truncated"] is False)


def transport(value, reason, quiescent):
    require(type(value) is dict and set(value) == {
        "kind", "reason", "phase", "locally_quiescent", "caller_context", "stop_observed"})
    require(value["kind"] == "transport" and value["reason"] == reason
            and value["locally_quiescent"] is quiescent)


def validate(events, source, nonce, exit_code):
    require(type(source) is str and re.fullmatch(r"[0-9a-f]{40}", source) is not None)
    require(type(nonce) is str and str(uuid.UUID(nonce)) == nonce)
    require(type(events) is list and len(events) == 5 and type(exit_code) is int and exit_code == 0)
    launches = 0
    for index, value in enumerate(events):
        require(type(value) is dict and set(value) == {
            "schema_version", "source_commit", "build_source_commit", "nonce", "policy", "case", "hostname",
            "state", "passed", "elapsed_milliseconds", "outcome", "probe", "followup"})
        require(type(value["schema_version"]) is int and value["schema_version"] == 1)
        require(value["source_commit"] == source == value["build_source_commit"] and value["nonce"] == nonce)
        require(value["policy"] == POLICY and value["case"] == CASES[index] and value["hostname"] == HOSTS[index])
        require(value["state"] == "observed" and value["passed"] is True)
        require(type(value["elapsed_milliseconds"]) is int and 0 <= value["elapsed_milliseconds"] <= 30_000)
        p = probe(value["probe"])
        require(p["trace_truncated"] is False)
        launches += len(p["launch_returns"])
        result = value["outcome"]
        if index < 2:
            transport(result, "cancelled" if index == 0 else "deadline", True)
            require(result["phase"] == "before_request" and result["caller_context"] is None and zero(p))
            require(result["stop_observed"] == ("cancelled" if index == 0 else "deadline"))
        elif index == 2:
            require(type(result) is dict and set(result) == {
                "kind", "candidate_count", "method", "authoritative_complete_set"})
            require(result["kind"] == "resolved" and result["method"] == "windows_completed_system_candidates"
                    and result["authoritative_complete_set"] is False)
            require(type(result["candidate_count"]) is int and 1 <= result["candidate_count"] <= 64)
            require(released(p) and p["results_freed"] == 1 and not p["cancel_returns"])
            require(p["launch_returns"] == [0] or (p["launch_returns"] == [997] and p["completion_returns"][-1] == 0))
        elif index == 3:
            require(result == {"kind": "stopped", "reason": "network"} and released(p) and not p["cancel_returns"])
            require(p["launch_returns"] in ([11001], [11004]) or
                    (p["launch_returns"] == [997] and p["completion_returns"][-1] in {11001, 11004}))
        else:
            transport(result, "quiescence_unverified", False)
            require(result["phase"] == "dns" and p["launch_returns"] == [997] and p["cancel_returns"])
            require(result["stop_observed"] == "cancelled")
            require(p["wsa_startup"] == [0] and p["events_created"] == 1 and p["contexts_created"] == 1)
            if result["caller_context"] == "released_after_completion":
                require(released(p))
            else:
                require(result["caller_context"] == "retained_pending_completion" and p["contexts_retained"] == 1)
                require(not p["completion_returns"] or p["completion_returns"][-1] in PENDING)
            followup = value["followup"]
            require(type(followup) is dict and set(followup) == {"outcome", "probe"})
            transport(followup["outcome"], "recovery_required", False)
            require(followup["outcome"]["phase"] == "before_request"
                    and followup["outcome"]["caller_context"] is None
                    and followup["outcome"]["stop_observed"] is None and zero(probe(followup["probe"])))
        if index != 4:
            require(value["followup"] is None)
    require(launches == 3)
    require(sum(value["elapsed_milliseconds"] for value in events) <= 30_000)
    return events
