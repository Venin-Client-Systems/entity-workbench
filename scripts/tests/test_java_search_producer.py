"""Offline producer controls; no Maven, JDK or worker invocation."""
import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_java_search as producer
import java_build_inputs as inputs


def completed():
    jar = {"sha256": "a" * 64, "bytes": 3, "members": []}
    return {"policy": producer.POLICY, "options": producer.OPTIONS,
            "build_goal": "package", "offline_is_network_sandbox": False,
            "builds": [{"number": n, "exit_code": 0, "jar": copy.deepcopy(jar),
                        "dependencies": [{"name": "dependency.jar", "sha256": "b" * 64, "bytes": 1}]}
                       for n in (1, 2)], "source_unchanged": True, "tool_inputs_unchanged": True,
            "cache_unchanged": True, "staged_search_unchanged": True,
            "staged_worker": {"sha256": "c" * 64}, "staged_worker_byte_equal": False}


class ContractTests(unittest.TestCase):
    def test_complete_reproduction_does_not_imply_staged_jar_provenance(self):
        value = completed()
        producer.validate_success(value)
        value["staged_worker_byte_equal"] = True
        with self.assertRaises(inputs.InvalidBuild):
            producer.validate_success(value)

    def test_incomplete_changed_and_mistyped_receipts_are_refused(self):
        mutations = [lambda v: v["builds"].pop(),
                     lambda v: v["builds"][1]["jar"].update(sha256="d" * 64),
                     lambda v: v["builds"][1]["dependencies"].clear(),
                     lambda v: v["builds"][0].update(exit_code=True),
                     lambda v: v["builds"][0].update(number=True),
                     lambda v: v.update(source_unchanged=False),
                     lambda v: v.update(cache_unchanged=1),
                     lambda v: v.update(staged_search_unchanged=False)]
        for mutate in mutations:
            value = completed()
            mutate(value)
            with self.assertRaises(inputs.InvalidBuild):
                producer.validate_success(value)

    def test_closed_command_and_environment_cannot_select_worker_or_online_goal(self):
        with patch.dict(os.environ, {"JAVA_TOOL_OPTIONS": "injected", "MAVEN_ARGS": "deploy"}):
            env = producer.environment(Path("jdk"), Path("maven"), Path("private"))
        self.assertNotIn("JAVA_TOOL_OPTIONS", env)
        self.assertNotIn("MAVEN_ARGS", env)
        self.assertEqual(env["MAVEN_SKIP_RC"], "true")
        args = producer.command(Path("maven"), Path("project"), Path("repository"), Path("settings"), Path("toolchains"))
        self.assertEqual(args[-1], "package")
        for expected in ("--offline", "-Dmaven.test.skip=true", "-Dmaven.compiler.proc=none",
                         "-Dcyclonedx.skip=true", "-Dproject.build.outputTimestamp=" + producer.TIMESTAMP):
            self.assertIn(expected, args)
        self.assertEqual(args.count("package"), 1)


@unittest.skipUnless(os.name == "posix", "Actual no-follow descriptor and private-mode controls require POSIX")
class PosixInputsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve()

    def tearDown(self):
        self.temp.cleanup()

    def test_copy_is_exact_no_clobber_and_refuses_drift(self):
        source = self.root / "source"
        source.mkdir()
        (source / "file").write_bytes(b"original")
        rows = inputs.scan(source)
        inputs.copy_verified(source, self.root / "copy", rows)
        self.assertEqual(inputs.scan(self.root / "copy"), rows)
        with self.assertRaises(FileExistsError):
            inputs.copy_verified(source, self.root / "copy", rows)
        (source / "file").write_bytes(b"modified")
        with self.assertRaises(inputs.InvalidBuild):
            inputs.copy_verified(source, self.root / "drift", rows)

    def test_links_special_files_and_bounds_are_refused(self):
        source = self.root / "input"
        source.write_bytes(b"abc")
        alias = self.root / "alias"
        alias.symlink_to(source)
        with self.assertRaises(inputs.InvalidBuild): inputs.file_bytes(alias)
        alias.unlink()
        os.link(source, alias)
        with self.assertRaises(inputs.InvalidBuild): inputs.file_bytes(source)
        alias.unlink()
        os.mkfifo(alias)
        with self.assertRaises(inputs.InvalidBuild): inputs.file_bytes(alias)
        alias.unlink()
        with self.assertRaises(inputs.InvalidBuild): inputs.scan(self.root, maximum_bytes=2)
        with self.assertRaises(inputs.InvalidBuild): inputs.scan(self.root, maximum_files=0)
        (self.root / "child").mkdir()
        with self.assertRaises(inputs.InvalidBuild): inputs.scan(self.root, maximum_depth=0)

    def test_inplace_mutation_is_not_accepted_as_stable_input(self):
        path = self.root / "input"
        path.write_bytes(b"first")
        original = os.read
        changed = False
        def read(fd, size):
            nonlocal changed
            data = original(fd, size)
            if data and not changed:
                changed = True
                path.write_bytes(b"later")
            return data
        with patch.object(inputs.os, "read", side_effect=read):
            with self.assertRaises(inputs.InvalidBuild): inputs.file_bytes(path)
        self.assertTrue(changed)

    def test_jar_identity_includes_raw_metadata_and_bounds_members(self):
        def archive(path, timestamp, name="workbench/SearchWorker.class", major=65):
            with zipfile.ZipFile(path, "w") as jar:
                jar.writestr(zipfile.ZipInfo(name, timestamp), b"\xca\xfe\xba\xbe\0\0" + major.to_bytes(2, "big"))
        a, b = self.root / "a.jar", self.root / "b.jar"
        archive(a, (2020, 1, 1, 0, 0, 0))
        archive(b, (2021, 1, 1, 0, 0, 0))
        first, second = inputs.jar_inventory(a), inputs.jar_inventory(b)
        self.assertEqual(first["members"], second["members"])
        self.assertNotEqual(first["sha256"], second["sha256"])
        with self.assertRaises(inputs.InvalidBuild):
            inputs.jar_inventory(b, (2020, 1, 1, 0, 0, 0))
        b.unlink(); archive(b, (2020, 1, 1, 0, 0, 0), "../escape")
        with self.assertRaises(inputs.InvalidBuild): inputs.jar_inventory(b)
        b.unlink(); archive(b, (2020, 1, 1, 0, 0, 0), major=69)
        with self.assertRaises(inputs.InvalidBuild): inputs.jar_inventory(b)

    def test_initial_source_failure_retains_private_receipt_and_never_executes(self):
        with patch.object(producer, "tracked_source", side_effect=producer.Failure("source_not_clean")), \
             patch.object(producer, "run_logged", side_effect=AssertionError("execution")):
            value = producer.observe(self.root, signers=self.root, jdk=self.root, maven=self.root,
                                     cache=self.root, staged=self.root)
        self.assertFalse(value["passed"])
        self.assertEqual(value["failure"], "source_not_clean")
        path = self.root / "receipt.json"
        self.assertEqual(json.loads(path.read_text()), value)
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_actual_plugin_headers_must_match_fixed_offline_build(self):
        log = self.root / "build.log"
        headers = "\n".join(f"--- {name[6:-7]}:{version}:goal (fixed) @ workers ---"
                            for name, version in producer.PLUGINS.items())
        log.write_text(headers + "\n--- cyclonedx:2.9.1:makeAggregateBom (fixed) @ workers ---\n")
        self.assertEqual(len(producer.plugin_headers(log)), 6)
        log.write_text(log.read_text().replace("jar:3.4.1:", "jar:3.4.2:"))
        with self.assertRaises(inputs.InvalidBuild): producer.plugin_headers(log)

    def test_primary_timeout_survives_log_hash_failure(self):
        with patch.object(producer, "tracked_source", side_effect=producer.ProcessTimeout({"process_exit_confirmed": False})), \
             patch.object(inputs, "file_bytes", side_effect=inputs.InvalidBuild("changed log")):
            (self.root / "retained.log").write_text("partial")
            value = producer.observe(self.root, signers=self.root, jdk=self.root, maven=self.root,
                                     cache=self.root, staged=self.root)
        self.assertFalse(value["passed"])
        self.assertEqual(value["failure"], "owned_build_process_timeout")
        self.assertIn("diagnostic_identity_unavailable", value["evidence_failures"])
        self.assertFalse(value["owned_process_stop"]["process_exit_confirmed"])

    def test_build_failure_stops_before_second_build_and_preserves_log(self):
        # File/tool identities are explicit synthetic seams; Maven is never invoked.
        (self.root / "release").write_text('JAVA_VERSION="25"\n')
        calls = []
        def execute(command, log, seconds, env):
            calls.append(command)
            log.write_text("synthetic offline resolution failure\n")
            return 1 if "package" in command else 0
        source = {"files": [{"path": "workers/java/pom.xml"}]}
        rows = [{"path": "pom.xml", "bytes": 1, "sha256": "a" * 64, "executable": False}]
        with patch.object(producer, "tracked_source", return_value=source), \
             patch.object(inputs, "scan", return_value=rows), \
             patch.object(inputs, "copy_verified"), \
             patch.object(inputs, "jar_inventory", return_value={"sha256": "b" * 64}), \
             patch.object(producer, "selected_plugins", return_value=[]), \
             patch.object(producer, "run_logged", side_effect=execute):
            value = producer.observe(self.root, signers=self.root, jdk=self.root, maven=self.root,
                                     cache=self.root, staged=self.root)
        self.assertEqual(value["failure"], "offline_maven_package_failed")
        self.assertEqual(len(value["builds"]), 1)
        self.assertEqual(sum("package" in call for call in calls), 1)
        self.assertTrue((self.root / "build-1.log").is_file())
        self.assertFalse((self.root / "build-2").exists())


if __name__ == "__main__":
    unittest.main()
