"""Synthetic inventory/classpath checks; these never execute Java or AppContainer."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location("stage_windows_engines", Path(__file__).parents[1] / "stage_windows_engines.py")
stage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stage)


class StagingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.java = self.root / "java"
        (self.java / "bin").mkdir(parents=True)
        (self.java / "bin/java.exe").write_bytes(b"synthetic executable")
        (self.java / "release").write_text('JAVA_VERSION="21.0.synthetic"\n')
        self.workers = self.root / "workers"
        (self.workers / "lib").mkdir(parents=True)
        with zipfile.ZipFile(self.workers / "workers-0.1.0.jar", "w") as jar:
            for name in ["Protocol", "FileWorker", "ParseWorker", "ParseWorker$BoundedWriter", "SearchWorker", "PdfRenderWorker"]:
                jar.writestr(f"workbench/{name}.class", b"synthetic class")
        for name in stage.REQUIRED["parser"] | stage.REQUIRED["search"]:
            (self.workers / "lib" / name).write_bytes(b"synthetic dependency")

    def test_roles_copy_only_reviewed_worker_classes_and_dependencies(self):
        destination = self.root / "staged"
        stage.stage(self.java, self.workers, destination)
        for role, own, forbidden in [("parser", "ParseWorker", "SearchWorker"), ("search", "SearchWorker", "ParseWorker")]:
            root = destination / role
            with zipfile.ZipFile(root / "worker.jar") as jar:
                self.assertIn(f"workbench/{own}.class", jar.namelist())
                self.assertNotIn(f"workbench/{forbidden}.class", jar.namelist())
                self.assertNotIn("workbench/PdfRenderWorker.class", jar.namelist())
            manifest = json.loads((root / "manifest.json").read_text())
            self.assertEqual(manifest["role"], role)
            self.assertTrue(manifest["development_only"])
            files, directories = stage.inventory(root)
            del files["manifest.json"]
            self.assertEqual(manifest["files"], files)
            self.assertEqual(manifest["directories"], directories)
        self.assertFalse(any(p.name.startswith("lucene-") for p in (destination / "parser/lib").iterdir()))
        self.assertFalse(any(p.name.startswith("tika-") for p in (destination / "search/lib").iterdir()))
        with self.assertRaises(ValueError):
            stage.stage(self.java, self.workers, destination)

    def test_missing_pins_and_wrong_java_never_produce_ready_bundles(self):
        (self.workers / "lib/pdfbox-3.0.8.jar").unlink()
        with self.assertRaises(ValueError):
            stage.stage(self.java, self.workers, self.root / "missing")
        self.assertFalse((self.root / "missing/parser/manifest.json").exists())
        (self.java / "release").write_text('JAVA_VERSION="17.0.synthetic"\n')
        with self.assertRaises(ValueError):
            stage.stage(self.java, self.workers, self.root / "wrong-java")

    def test_hardlinks_and_unsafe_portable_names_are_rejected(self):
        alias = self.java / "alias.exe"
        alias.hardlink_to(self.java / "bin/java.exe")
        with self.assertRaises(ValueError):
            stage.inventory(self.java)
        for name in ["../escape", "a/b:stream", "a\\b", "CON.txt", "NUL", "LPT1.jar", "a.", ".hidden"]:
            self.assertFalse(stage.safe_name(name), name)

    def test_oversized_selected_class_is_rejected_before_ready_manifest(self):
        with zipfile.ZipFile(self.workers / "workers-0.1.0.jar", "w", compression=zipfile.ZIP_DEFLATED) as jar:
            for name in ["Protocol", "FileWorker", "ParseWorker", "SearchWorker"]:
                jar.writestr(f"workbench/{name}.class", b"x" * (1024 * 1024 + 1) if name == "Protocol" else b"synthetic")
        destination = self.root / "oversized"
        with self.assertRaises(ValueError):
            stage.stage(self.java, self.workers, destination)
        self.assertFalse((destination / "parser/manifest.json").exists())
        self.assertFalse((destination / "development-only.json").exists())

    def test_duplicate_class_entries_are_rejected(self):
        import warnings
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", UserWarning)
            with zipfile.ZipFile(self.workers / "workers-0.1.0.jar", "a") as jar:
                jar.writestr("workbench/Protocol.class", b"duplicate")
        with self.assertRaises(ValueError):
            stage.stage(self.java, self.workers, self.root / "duplicate")


if __name__ == "__main__":
    unittest.main()
