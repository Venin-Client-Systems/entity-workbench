"""Stage a separate development parser JAR/classpath from already built local assets.

No downloads. Pass a Maven worker target containing workers-0.1.0.jar and lib/.
The existing staged Java runtime is reused; this is not a complete release bundle.
"""
import argparse
from pathlib import Path
import shutil
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker-target", type=Path, default=ROOT / "workers/java/target")
    parser.add_argument("--runtime", type=Path, default=ROOT / "runtime/staged/engines")
    args = parser.parse_args()
    if not (args.runtime / "java/bin/java").is_file():
        parser.error("Stage the existing Java runtime first")
    source = args.worker_target.resolve(strict=True)
    target = args.runtime / "parser"
    if target.exists():
        parser.error("Parser directory already exists; use a fresh staging destination")
    jar = source / "workers-0.1.0.jar"
    with zipfile.ZipFile(jar) as archive:
        names = [name for name in archive.namelist() if name in ("workbench/Protocol.class", "workbench/ParseWorker.class", "workbench/AppLocalFonts.class", "workbench/AppLocalFonts$AssetException.class", "workbench/HostileProbe.class", "workbench/HostileProbe$Attempt.class") or (name.startswith("workbench/ParseWorker$") and name.endswith(".class"))]
        if not {"workbench/ParseWorker.class", "workbench/Protocol.class", "workbench/AppLocalFonts.class", "workbench/AppLocalFonts$AssetException.class"}.issubset(names):
            parser.error("Current parser classes are missing from the worker JAR")
        (target / "lib").mkdir(parents=True)
        with zipfile.ZipFile(target / "workers-0.1.0.jar", "w", compression=zipfile.ZIP_DEFLATED) as output:
            for name in names:
                output.writestr(name, archive.read(name))
    for dependency in sorted((source / "lib").glob("*.jar")):
        if not dependency.name.startswith("lucene-"):
            shutil.copy2(dependency, target / "lib" / dependency.name)
    print("Staged development parser classes and local dependencies; Lucene excluded")


if __name__ == "__main__":
    main()
