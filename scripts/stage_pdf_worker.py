"""Compile and stage the narrow PDF renderer from existing local JDK/JAR assets; no downloads."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DEPENDENCIES = (
    "pdfbox-3.0.8.jar", "pdfbox-io-3.0.8.jar", "fontbox-3.0.8.jar",
    "commons-logging-1.4.0.jar", "jackson-annotations-2.22.jar",
    "jackson-core-2.22.3.jar", "jackson-databind-2.22.3.jar",
)
NOTICE_NAMES = {"LICENSE", "NOTICE", "LICENSE.TXT", "NOTICE.TXT"}


def copy_dependency(path, target):
    shutil.copyfile(path, target / "lib" / path.name)
    found_license = False
    with zipfile.ZipFile(path) as archive:
        for name in archive.namelist():
            basename = name.rsplit("/", 1)[-1]
            if name.upper().startswith("META-INF/") and basename.upper() in NOTICE_NAMES:
                notice = target / "notices" / path.stem / basename
                notice.parent.mkdir(parents=True, exist_ok=True)
                notice.write_bytes(archive.read(name))
                found_license |= basename.upper().startswith("LICENSE")
    if not found_license:
        raise ValueError("A required dependency licence is missing")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--javac", type=Path, required=True)
    parser.add_argument("--java-runtime", type=Path, required=True)
    parser.add_argument("--dependencies", type=Path, required=True)
    parser.add_argument("--runtime", type=Path, default=ROOT / "runtime/staged/engines")
    args = parser.parse_args()
    target = args.runtime / "pdf-render"
    if target.exists():
        parser.error("Use a fresh PDF component staging destination")
    jars = [args.dependencies / name for name in DEPENDENCIES]
    if not all(path.is_file() for path in jars):
        parser.error("Required cached PDFBox/Jackson JARs are missing")
    java = args.java_runtime.resolve(strict=True)
    if not (java / "bin/java").is_file():
        parser.error("Existing Java runtime required")
    with tempfile.TemporaryDirectory() as temporary:
        classes = Path(temporary) / "classes"
        classes.mkdir()
        sources = [ROOT / "workers/java/src/main/java/workbench" / name for name in (
            "Protocol.java", "PdfRenderWorker.java", "PdfRenderPolicy.java", "HostileProbe.java")]
        subprocess.run([str(args.javac), "--release", "21", "-cp", ":".join(map(str, jars)),
                        "-d", str(classes), *map(str, sources)], check=True)
        if not (args.runtime / "java").exists():
            shutil.copytree(java, args.runtime / "java")
        (target / "lib").mkdir(parents=True)
        with zipfile.ZipFile(target / "workers-0.1.0.jar", "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(classes.rglob("*.class")):
                archive.write(path, str(path.relative_to(classes)))
        for path in jars:
            copy_dependency(path, target)
        inventory = {str(path.relative_to(target)): hashlib.sha256(path.read_bytes()).hexdigest()
                     for path in sorted(target.rglob("*")) if path.is_file()}
        (target / "manifest.json").write_text(json.dumps({
            "schema_version": 1, "renderer": "pdfbox-3.0.8-scan-v1", "files": inventory,
        }, indent=2) + "\n")
    print("Staged separate scan-focused PDFBox classpath with seven cached dependencies and notices; no downloads")


if __name__ == "__main__":
    main()
