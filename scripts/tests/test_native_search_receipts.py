"""Offline receipt/inventory controls. No candidate interpreter or Java execution."""
import copy
from contextlib import ExitStack
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import native_search_receipt as receipt
import native_search_runtime as runtime
import test_native_coordinator_search as runner


SOURCE = "a" * 40
RUNTIME = {"sha256": "b" * 64, "files": [["java/bin/java", 4, "c" * 64, True]]}


def events(nonce):
    output = []
    def event(kind, details):
        output.append({"policy": receipt.POLICY, "source": SOURCE, "nonce": nonce,
                       "runtime_sha256": RUNTIME["sha256"], "kind": kind, "details": details})
    event("start", {"recipe_entry_limit": 5, "worker_seconds": 30, "queries": receipt.QUERIES, "fixtures": receipt.FIXTURES})
    event("runtime_verified", RUNTIME)
    start = {"workspace_revision": 3, "canonical_sha256": "d" * 64, "originals": receipt.ORIGINALS}
    event("healthy_start", start)
    snapshot = {"files": [["segments_1", 1, 2, 7, "e" * 64]], "logical_bytes": 7,
                "marker": [1, 3, 1, hashlib.sha256(b"3").hexdigest()]}
    afters = []
    for i, query in enumerate(receipt.QUERIES):
        event("query_before", {"sequence": i, "query": query, "recipe_entries": receipt.RECIPES[:0 if i == 0 else i+1]})
        if i == 2:
            result = {"error": "validation"}
        else:
            fixture = receipt.FIXTURES[{0: 0, 1: 2, 3: 1}[i]]
            result = {"workspace_revision": "3", "total": 1,
                      "hits": [{"id": fixture["sha256"], "name": fixture["name"], "score": 1.25}]}
        after = {"sequence": i, "query": query, "result": result, "recipe_entries": receipt.RECIPES[:i+2],
                 "index": copy.deepcopy(snapshot), "canonical_unchanged": True,
                 "originals_unchanged": True, "assignment_cleanup": True}
        event("query_after", after)
        afters.append(after)
    healthy = dict(start, queries=afters, recipe_entries=receipt.RECIPES, shutdown_joined=True, ownership_released=True)
    event("healthy_complete", healthy)
    refusal = {"host_injected": True, "unknown_child_created": False, "search_error": "blocked",
               "recipe_entries": [], "canonical_unchanged": True, "canonical_sha256": "f" * 64,
               "workspace_revision": 3, "originals": receipt.ORIGINALS,
               "intent_unchanged": True, "index_unchanged": True, "shutdown_joined": True,
               "shutdown_quarantined": True, "ownership_released": False}
    for key, data in (("intent_identity", b"host-injected retained execution intent; no unknown process created by this control\n"),
                      ("index_sentinel_identity", b"unmodified synthetic derivative")):
        refusal[key] = [1, 9, len(data), hashlib.sha256(data).hexdigest()]
    event("intent_complete", refusal)
    event("final", {"passed": True, "healthy": healthy, "retained_intent": refusal,
                    "runtime_unchanged": True, "runtime_sha256": RUNTIME["sha256"], "recipe_entries": receipt.RECIPES,
                    "native_scope": "macos_selected_development_runtime"})
    # Separate nested object aliases, as a native JSON decoding would.
    return json.loads(json.dumps(output))


class ReceiptTests(unittest.TestCase):
    def setUp(self):
        self.nonce = str(uuid.uuid4())
        self.valid = events(self.nonce)

    def accept(self, value, code=0):
        return receipt.validate(value, SOURCE, self.nonce, RUNTIME, code)

    def test_complete_control_and_exact_cross_language_fixture_identity(self):
        self.assertEqual(self.accept(self.valid)["recipe_entry_count"], 5)
        self.assertFalse(self.accept(self.valid)["recipe_entries_are_spawn_evidence"])
        source = (runner.ROOT / "crates/core/src/coordinator_search_native.rs").read_text()
        for name, data in receipt.TEXTS:
            self.assertIn(name, source)
            self.assertIn(data.decode().strip(), source)
        self.assertEqual(len(self.valid), 14)

    def test_missing_reordered_extra_failed_or_foreign_receipts_are_refused(self):
        for changed in (self.valid[:-1], self.valid + [self.valid[-1]], list(reversed(self.valid))):
            with self.assertRaises(receipt.InvalidReceipt): self.accept(changed)
        for field, value in (("nonce", str(uuid.uuid4())), ("source", "0" * 40), ("runtime_sha256", "0" * 64), ("extra", True)):
            changed = copy.deepcopy(self.valid)
            changed[3][field] = value
            with self.assertRaises(receipt.InvalidReceipt): self.accept(changed)
        for code in (1, -9, False):
            with self.assertRaises(receipt.InvalidReceipt): self.accept(self.valid, code)

    def test_result_substitution_revision_bool_and_index_rebuild_are_refused(self):
        mutations = [
            lambda e: e[4]["details"]["result"]["hits"][0].update(id="0" * 64),
            lambda e: e[4]["details"]["result"].update(total=True),
            lambda e: e[4]["details"]["result"].update(workspace_revision="4"),
            lambda e: e[4]["details"].update(assignment_cleanup=False),
            lambda e: e[6]["details"]["index"]["files"][0].__setitem__(2, 99),
            lambda e: e[8]["details"].update(result={"error": "termination_unverified"}),
            lambda e: e[11]["details"].update(ownership_released=1),
            lambda e: e[12]["details"].update(ownership_released=True),
            lambda e: e[13]["details"].update(passed=1),
            lambda e: e[2]["details"]["originals"][0]["anchor"].update(line_start=True),
        ]
        for mutate in mutations:
            changed = copy.deepcopy(self.valid)
            mutate(changed)
            with self.assertRaises(receipt.InvalidReceipt): self.accept(changed)

    def test_duplicate_unknown_and_oversized_event_tail_retains_prefix(self):
        first = receipt.PREFIX + json.dumps(self.valid[0]) + "\n"
        for tail in ('{"kind":"x","kind":"y"}', '{"unfinished":', '[1]', '{"bad":NaN}', 'x' * (256 * 1024 + 1)):
            with self.assertRaises(receipt.EventDecodeFailure) as caught:
                receipt.decode(first + receipt.PREFIX + tail)
            self.assertEqual(caught.exception.events, [self.valid[0]])


@unittest.skipUnless(os.name == "posix", "POSIX-only development inventory; import remains portable")
class RuntimeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve()
        for name, data in (("java/bin/java", b"java"), ("java/release", b"release"),
                           ("search/workers-0.1.0.jar", b"worker"), ("search/lib/a.jar", b"dependency")):
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        (self.root / "java/bin/java").chmod(0o700)

    def tearDown(self): self.temp.cleanup()

    def test_exact_inventory_digest_and_changed_bytes_or_executable(self):
        original = runtime.inventory(self.root)
        self.assertEqual(original["file_count"], 4)
        self.assertEqual(original["sha256"], runtime.canonical_digest(original["files"]))
        (self.root / "search/lib/a.jar").write_bytes(b"changed")
        self.assertNotEqual(original["sha256"], runtime.inventory(self.root)["sha256"])
        (self.root / "java/bin/java").chmod(0o600)
        with self.assertRaises(runtime.InvalidRuntime): runtime.inventory(self.root)

    def test_links_alias_and_missing_components_are_refused(self):
        alias = self.root / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(runtime.InvalidRuntime): runtime.inventory(alias)
        alias.unlink()
        member = self.root / "search/lib/other"
        member.symlink_to(self.root / "java/release")
        with self.assertRaises((runtime.InvalidRuntime, OSError)): runtime.inventory(self.root)
        member.unlink()
        os.link(self.root / "java/release", member)
        with self.assertRaises(runtime.InvalidRuntime): runtime.inventory(self.root)
        member.unlink()
        (self.root / "java/release").unlink()
        with self.assertRaises(runtime.InvalidRuntime): runtime.inventory(self.root)

    def test_declared_oversize_depth_count_and_special_files(self):
        path = self.root / "search/lib/big"
        with path.open("wb") as stream: stream.truncate(128 * 1024 * 1024 + 1)
        with self.assertRaises(runtime.InvalidRuntime): runtime.inventory(self.root)
        path.unlink()
        os.mkfifo(path)
        with self.assertRaises(runtime.InvalidRuntime): runtime.inventory(self.root)
        path.unlink()
        deep = self.root / "search"
        for _ in range(8): deep = deep / "d"
        deep.mkdir(parents=True)
        with self.assertRaises(runtime.InvalidRuntime): runtime.inventory(self.root)
        import shutil
        shutil.rmtree(self.root / "search/d")
        for n in range(1024): (self.root / "search" / str(n)).touch()
        with self.assertRaises(runtime.InvalidRuntime): runtime.inventory(self.root)

    def test_inplace_content_mutation_during_hash_is_refused(self):
        original_read = os.read
        changed = False
        def read(fd, count):
            nonlocal changed
            block = original_read(fd, count)
            if block == b"worker" and not changed:
                changed = True
                (self.root / "search/workers-0.1.0.jar").write_bytes(b"mutate")
            return block
        with patch.object(runtime.os, "read", side_effect=read):
            with self.assertRaises(runtime.InvalidRuntime): runtime.inventory(self.root)
        self.assertTrue(changed)


class RunnerTests(unittest.TestCase):
    def run_case(self, *, approved=True, timeout=False, malformed=False, exit_code=0, changed_binary=False):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            artifact = root / str(uuid.uuid4())
            artifact.mkdir()
            binary = root / "test-binary"
            binary.write_bytes(b"trusted synthetic no-exec binary")
            source = {"revision": SOURCE}
            def native(command, log, seconds, environment):
                self.assertEqual(seconds, 180)
                self.assertEqual(command[1], receipt.TEST)
                self.assertEqual(environment["EW_NATIVE_SEARCH_NONCE"], artifact.name)
                actual = events(artifact.name)
                text = "\n".join(receipt.PREFIX + json.dumps(e) for e in actual)
                if malformed:
                    text = receipt.PREFIX + json.dumps(actual[0]) + "\n" + receipt.PREFIX + '{"tail":'
                log.write_text(text)
                if timeout:
                    raise runner.ProcessTimeout({"kill_signal_sent": False, "process_exit_confirmed": False, "exit_code": None, "errors": ["process_group_kill_failed", "process_reap_timeout"]})
                if changed_binary: binary.write_bytes(b"changed")
                return exit_code
            with ExitStack() as stack:
                for name, value in (("platform.system", "Darwin"), ("platform.machine", "arm64"), ("platform.mac_ver", ("test", (), "")),
                                    ("identity", source), ("text_command", "rustc synthetic"), ("build_binary", binary)):
                    stack.enter_context(patch("test_native_coordinator_search." + name, return_value=value))
                inventory = stack.enter_context(patch.object(runner.runtime_inventory, "inventory", return_value=RUNTIME))
                execute = stack.enter_context(patch.object(runner, "run_logged", side_effect=native))
                report = runner.observe(artifact, approved, root / "signers", root / "runtime", RUNTIME["sha256"])
                saved = json.loads((artifact / "report.json").read_text())
                self.assertEqual(saved, report)
                return report, inventory.call_count, execute.call_count

    def test_exact_success_and_initial_refusal_are_retained(self):
        report, inventories, executions = self.run_case()
        self.assertTrue(report["passed"])
        self.assertEqual((inventories, executions), (2, 1))
        report, inventories, executions = self.run_case(approved=False)
        self.assertFalse(report["passed"])
        self.assertEqual((inventories, executions), (0, 0))
        self.assertEqual(report["phase"], "authorization")

    def test_timeout_and_nonzero_exit_do_not_allow_runtime_postreads_or_retry(self):
        for options in ({"timeout": True}, {"exit_code": 1}, {"malformed": True}):
            report, inventories, executions = self.run_case(**options)
            self.assertFalse(report["passed"])
            self.assertFalse(report["runtime_postread_permitted"])
            self.assertEqual((inventories, executions), (1, 1))
            self.assertTrue(report["events"])
        report, _, _ = self.run_case(timeout=True)
        self.assertEqual(report["failure"], "outer_process_timeout")
        self.assertEqual(report["java_termination"], "unverified")
        self.assertFalse(report["owned_process_termination"]["process_exit_confirmed"])

    def test_later_binary_mutation_cannot_leave_prior_success(self):
        report, _, _ = self.run_case(changed_binary=True)
        self.assertFalse(report["passed"])
        self.assertEqual(report["failure"], "binary_changed_during_observation")


if __name__ == "__main__":
    unittest.main()
