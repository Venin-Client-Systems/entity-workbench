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

    def test_exact_lifecycle_and_explicit_offline_bom_skip_are_required(self):
        # Portable log guard test, without descriptor helpers or a Maven run.
        lines = [f"[INFO] --- {name}:{version}:{goal} (fixed) @ workers ---"
                 for name, version, goal in producer.LIFECYCLE]
        valid = "\n".join([*lines, producer.OFFLINE_BOM_SKIP]) + "\n"
        with patch.object(inputs, "file_bytes", return_value=valid.encode()):
            self.assertEqual(producer.plugin_headers(Path("unused")),
                             [list(row) for row in producer.LIFECYCLE])
        invalid = {
            "missing lifecycle plugin": valid.replace(lines[-1] + "\n", ""),
            "wrong plugin version": valid.replace("jar:3.4.1:", "jar:3.4.2:"),
            "wrong goal": valid.replace("copy-dependencies", "resolve"),
            "unexpected execution": valid + "[INFO] --- exec:3.5.0:java (extra) @ workers ---\n",
            "unexpected malformed header": valid + "[INFO] --- unparsed-extra-execution ---\n",
            "CycloneDX executed": valid + "[INFO] --- cyclonedx:2.9.1:makeAggregateBom (extra) @ workers ---\n",
            "duplicate lifecycle": valid + lines[0] + "\n",
            "missing offline skip": "\n".join(lines),
            "wrong offline skip": valid.replace("makeAggregateBom", "anotherGoal"),
            "duplicate offline skip": valid + producer.OFFLINE_BOM_SKIP + "\n",
        }
        for label, value in invalid.items():
            with self.subTest(label=label), patch.object(inputs, "file_bytes", return_value=value.encode()):
                with self.assertRaises(inputs.InvalidBuild):
                    producer.plugin_headers(Path("unused"))


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
        source = {"files": [{"path": "workers/java/pom.xml"}]}
        rows = [{"path": "pom.xml", "bytes": 1, "sha256": "a" * 64, "executable": False}]
        for exit_code, failure in [(1, "offline_maven_package_failed"),
                                   (0, "actual_build_plugin_versions_differ")]:
            with self.subTest(exit_code=exit_code):
                artifact = self.root / str(exit_code)
                artifact.mkdir()
                calls = []
                def execute(command, log, seconds, env):
                    calls.append(command)
                    log.write_text("synthetic incomplete build diagnostic\n")
                    return exit_code if "package" in command else 0
                with patch.object(producer, "tracked_source", return_value=source), \
                     patch.object(inputs, "scan", return_value=rows), \
                     patch.object(inputs, "copy_verified"), \
                     patch.object(inputs, "jar_inventory", return_value={"sha256": "b" * 64}), \
                     patch.object(producer, "selected_plugins", return_value=[]), \
                     patch.object(producer, "run_logged", side_effect=execute):
                    value = producer.observe(artifact, signers=self.root, jdk=self.root, maven=self.root,
                                             cache=self.root, staged=self.root)
                self.assertEqual(value["failure"], failure)
                self.assertEqual(value["builds"], [{"number": 1, "exit_code": exit_code}])
                self.assertEqual(sum("package" in call for call in calls), 1)
                self.assertTrue((artifact / "build-1.log").is_file())
                self.assertFalse((artifact / "build-2").exists())


if __name__ == "__main__":
    unittest.main()
