"""Synthetic preparation controls only; no Java/Maven/compiler/worker invocation."""
import copy
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import prepare_java_search_candidate as candidate
import java_build_inputs as inputs
import native_search_runtime as runtime
import build_java_search as producer


def producer_report(body=b"new worker"):
    jar = inputs.identity(body)
    return {"policy": producer.POLICY, "options": producer.OPTIONS, "build_goal": "package",
            "offline_is_network_sandbox": False, "passed": True, "phase": "complete", "failure": None,
            "source": {"revision": "a" * 40, "tree": "b" * 40},
            "builds": [{"number": n, "exit_code": 0, "jar": copy.deepcopy(jar),
                        "dependencies": [], "search_dependencies": []} for n in (1, 2)],
            "source_unchanged": True, "tool_inputs_unchanged": True, "cache_unchanged": True,
            "staged_search_unchanged": True, "staged_worker": {"sha256": "c" * 64},
            "staged_worker_byte_equal": False}


def evidence(report):
    raw = json.dumps(report).encode()
    return raw, {"artifact_id": "fixture", "source": report["source"],
                 "artifacts": [{"path": "artifacts/java-search-producer/fixture/receipt.json",
                                **inputs.identity(raw)}]}


class BindingTests(unittest.TestCase):
    def test_exact_successful_producer_identity_required(self):
        report = producer_report()
        raw, approved = evidence(report)
        with patch.object(candidate, "WORKER_PIN", report["builds"][0]["jar"]["sha256"]):
            self.assertEqual(candidate.bound_receipt(raw, approved), report)
            with self.assertRaises(inputs.InvalidBuild):
                candidate.bound_receipt(raw + b" ", approved)
            changed = copy.deepcopy(approved)
            changed["source"]["revision"] = "d" * 40
            with self.assertRaises(inputs.InvalidBuild):
                candidate.bound_receipt(raw, changed)
            for mutate in (lambda r: r.update(passed=False), lambda r: r["builds"].pop(),
                           lambda r: r["builds"][0].update(exit_code=True)):
                failed = copy.deepcopy(report)
                mutate(failed)
                with self.assertRaises(inputs.InvalidBuild):
                    candidate.bound_receipt(*evidence(failed))

    def test_duplicate_keys_and_nonfinite_numbers_refused(self):
        for value in (b'{"passed":false,"passed":true}', b'{"value":NaN}', b'{"nested":{"a":1,"a":2}}'):
            with self.assertRaises(inputs.InvalidBuild):
                candidate.strict_json(value)


@unittest.skipUnless(os.name == "posix", "Actual no-follow copy/private-mode guarantees require POSIX")
class CopyTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve()
        self.source = self.root / "source"
        for name, value in {"java/bin/java": b"synthetic executable", "java/release": b"JAVA_VERSION=21",
                            candidate.WORKER: b"old worker", **{f"search/lib/d{i}.jar": str(i).encode() for i in range(9)}}.items():
            path = self.source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(value)
        (self.source / "java/bin/java").chmod(0o700)
        self.before = runtime.inventory(self.source)
        self.body = b"new worker"
        self.producer = producer_report(self.body)
        self.producer["builds"][0]["search_dependencies"] = [
            {"name": r[0][11:], "bytes": r[1], "sha256": r[2]}
            for r in self.before["files"] if r[0].startswith("search/lib/")]
        self.pins = patch.multiple(candidate, RUNTIME_PIN=self.before["sha256"],
                                   WORKER_PIN=hashlib.sha256(self.body).hexdigest())
        self.pins.start()
        self.rows = candidate.candidate_rows(self.before, self.producer, self.body)

    def tearDown(self):
        self.pins.stop()
        self.temporary.cleanup()

    def test_only_worker_changes_with_private_no_clobber_copy(self):
        target = self.root / "candidate"
        candidate.copy_candidate(self.source, target, self.rows, self.body)
        after = candidate.verify_candidate(target, self.rows)
        self.assertEqual([a[0] for a, b in zip(self.before["files"], after["files"]) if a != b], [candidate.WORKER])
        self.assertEqual(runtime.inventory(self.source), self.before)
        self.assertEqual((target / "java/bin/java").stat().st_mode & 0o777, 0o700)
        self.assertEqual((target / candidate.WORKER).stat().st_mode & 0o777, 0o600)
        with self.assertRaises(FileExistsError):
            candidate.copy_candidate(self.source, target, self.rows, self.body)
        self.assertEqual(candidate.verify_candidate(target, self.rows), after)

    def test_source_drift_links_hardlinks_and_destination_alias_fail(self):
        path = self.source / "java/release"
        path.write_bytes(b"changed")
        with self.assertRaises(inputs.InvalidBuild):
            candidate.copy_candidate(self.source, self.root / "drift", self.rows, self.body)
        path.unlink()
        path.symlink_to(self.source / "java/bin/java")
        with self.assertRaises(inputs.InvalidBuild):
            candidate.copy_candidate(self.source, self.root / "linked", self.rows, self.body)
        path.unlink()
        os.link(self.source / "java/bin/java", path)
        with self.assertRaises(inputs.InvalidBuild):
            candidate.copy_candidate(self.source, self.root / "hardlink", self.rows, self.body)
        alias = self.root / "alias"
        alias.symlink_to(self.source, target_is_directory=True)
        with self.assertRaises(inputs.InvalidBuild):
            candidate.copy_candidate(self.source, alias / "never", self.rows, self.body)
        self.assertFalse((self.source / "never").exists())

    def test_wrong_pin_worker_dependencies_and_extra_search_file_fail(self):
        changes = [lambda b: b.update(sha256="e" * 64),
                   lambda b: b["files"].append(["search/extra", 1, "f" * 64, False]),
                   lambda b: b["files"].pop(),
                   lambda b: b["files"][0].__setitem__(1, 256 * 1024 * 1024)]
        for change in changes:
            altered = copy.deepcopy(self.before)
            change(altered)
            with self.assertRaises(inputs.InvalidBuild):
                candidate.candidate_rows(altered, self.producer, self.body)
        with self.assertRaises(inputs.InvalidBuild):
            candidate.candidate_rows(self.before, self.producer, b"wrong body")

    def test_unexpected_directory_or_file_prevents_verification(self):
        target = self.root / "candidate"
        candidate.copy_candidate(self.source, target, self.rows, self.body)
        extra = target / "java/empty-extra"
        extra.mkdir()
        with self.assertRaises(inputs.InvalidBuild): candidate.verify_candidate(target, self.rows)
        extra.rmdir()
        (target / "unrelated").write_bytes(b"retain")
        with self.assertRaises(inputs.InvalidBuild): candidate.verify_candidate(target, self.rows)
        self.assertEqual((target / "unrelated").read_bytes(), b"retain")

    def test_incomplete_source_receipt_retained_and_no_copy_attempted(self):
        artifact = self.root / "failed"
        artifact.mkdir()
        with patch.object(producer, "tracked_source", side_effect=producer.Failure("source_not_clean")), \
             patch.object(candidate, "copy_candidate", side_effect=AssertionError("unexpected copy")):
            result = candidate.prepare(artifact, self.source, self.root, self.root)
        self.assertFalse(result["prepared"])
        self.assertEqual(result["failure"], "source_not_clean")
        self.assertFalse(json.loads((artifact / "initial.json").read_bytes())["prepared"])
        self.assertEqual(json.loads((artifact / "receipt.json").read_bytes()), result)
        self.assertFalse((artifact / "runtime").exists())

    def test_failed_final_verification_retains_inert_partial_copy(self):
        artifact = self.root / "partial"
        artifact.mkdir()
        real_copy = candidate.copy_candidate
        def changed_copy(source, target, rows, body):
            real_copy(source, target, rows, body)
            (target / "unexpected").write_bytes(b"retain unrelated bytes")
        with patch.object(producer, "tracked_source", return_value={"revision": "a" * 40}), \
             patch.object(candidate, "producer_input", return_value=(self.producer, b"receipt", self.body)), \
             patch.object(candidate, "copy_candidate", side_effect=changed_copy):
            result = candidate.prepare(artifact, self.source, self.root, self.root)
        self.assertFalse(result["prepared"])
        self.assertEqual(result["failure"], "unexpected_candidate_root_entry")
        self.assertEqual((artifact / "runtime/unexpected").read_bytes(), b"retain unrelated bytes")
        self.assertFalse(json.loads((artifact / "initial.json").read_bytes())["prepared"])
        self.assertFalse(json.loads((artifact / "receipt.json").read_bytes())["prepared"])
        self.assertEqual(runtime.inventory(self.source), self.before)


if __name__ == "__main__":
    unittest.main()
