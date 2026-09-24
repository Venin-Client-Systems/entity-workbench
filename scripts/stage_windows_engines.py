"""Stage separate development Java parser/search runtimes from local built assets.

No downloads or runtime execution. The caller supplies Java 21 and the existing
pinned Maven target. This is a test bundle, not a Windows release distribution.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import stat
import zipfile

MAX_BYTES = 1024 * 1024 * 1024
MAX_MEMBERS = 10000
FONT_RESOURCE = "org/apache/pdfbox/resources/ttf/LiberationSans-Regular.ttf"
FONT_BYTES = 410712
FONT_SHA256 = "76d04c18ea243f426b7de1f3ad208e927008f961dc5945e5aad352d0dfde8ee8"
STANDARD_AFM = {"Courier", "Courier-Bold", "Courier-Oblique", "Courier-BoldOblique",
                "Helvetica", "Helvetica-Bold", "Helvetica-Oblique", "Helvetica-BoldOblique",
                "Times-Roman", "Times-Bold", "Times-Italic", "Times-BoldItalic", "Symbol", "ZapfDingbats"}
REQUIRED = {
    "parser": {"tika-core-3.3.2.jar", "tika-parser-microsoft-module-3.3.2.jar", "pdfbox-3.0.8.jar", "jackson-databind-2.22.3.jar"},
    "search": {"lucene-core-10.5.1.jar", "lucene-analysis-common-10.5.1.jar", "lucene-queryparser-10.5.1.jar", "jackson-databind-2.22.3.jar"},
}


def ordinary(path):
    info = path.lstat()
    if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
        raise ValueError("Link or reparse asset rejected")
    if not (stat.S_ISDIR(info.st_mode) or stat.S_ISREG(info.st_mode)):
        raise ValueError("Special asset rejected")
    if stat.S_ISREG(info.st_mode) and info.st_nlink != 1:
        raise ValueError("Hardlinked asset rejected")
    return info


def safe_name(value):
    parts = value.split("/")
    return len(value) <= 512 and all(
        re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}", p)
        and not p.endswith(".")
        and p.split(".", 1)[0].upper() not in {"CON", "PRN", "AUX", "NUL", *(f"COM{i}" for i in range(1, 10)), *(f"LPT{i}" for i in range(1, 10))}
        for p in parts
    )


def directory_chain(root, allow_missing=False):
    if ".." in root.parts:
        raise ValueError("Staging path traversal rejected")
    root = root.absolute()
    for path in [*reversed(root.parents), root]:
        try:
            info = ordinary(path)
        except FileNotFoundError:
            if allow_missing:
                continue
            raise
        if not stat.S_ISDIR(info.st_mode):
            raise ValueError("Staging ancestor is not a directory")


def inventory(root):
    root = root.absolute()
    directory_chain(root)
    files, directories, seen = {}, [], set()
    total = 0
    pending = [(root, 0)]
    while pending:
        parent, depth = pending.pop()
        if depth > 16:
            raise ValueError("Runtime depth exceeded")
        for path in sorted(parent.iterdir()):
            name = path.relative_to(root).as_posix()
            if not safe_name(name) or name.casefold() in seen:
                raise ValueError("Unsafe or colliding runtime name")
            seen.add(name.casefold())
            if len(seen) > MAX_MEMBERS:
                raise ValueError("Runtime member bound exceeded")
            info = ordinary(path)
            if stat.S_ISDIR(info.st_mode):
                directories.append(name)
                pending.append((path, depth + 1))
            else:
                total += info.st_size
                if total > MAX_BYTES:
                    raise ValueError("Runtime size exceeded")
                with path.open("rb") as source:
                    digest = hashlib.file_digest(source, "sha256").hexdigest()
                files[name] = {"bytes": info.st_size, "sha256": digest}
    return dict(sorted(files.items())), sorted(directories)


def verify_pdfbox_assets(path):
    # The full JAR remains inventoried. Bound each inspected embedded asset before
    # decompression, and preserve the original font/AFM licences and notices.
    with zipfile.ZipFile(path) as jar:
        names = jar.namelist()
        if len(names) != len(set(names)):
            raise ValueError("Duplicate PDFBox resource")
        required = {FONT_RESOURCE, "META-INF/LICENSE", "META-INF/NOTICE"} | {
            f"org/apache/pdfbox/resources/afm/{name}.afm" for name in STANDARD_AFM}
        if not required.issubset(names):
            raise ValueError("Required app-local font resources or notices absent")
        for name in sorted(required):
            member = jar.getinfo(name)
            limit = FONT_BYTES if name == FONT_RESOURCE else 128 * 1024
            if member.file_size > limit or member.file_size < 1:
                raise ValueError("App-local font resource exceeds bound")
            with jar.open(member) as source:
                content = source.read(limit + 1)
            if len(content) != member.file_size or len(content) > limit:
                raise ValueError("App-local font resource size mismatch")
            if name == FONT_RESOURCE and (len(content) != FONT_BYTES or hashlib.sha256(content).hexdigest() != FONT_SHA256):
                raise ValueError("App-local font resource digest mismatch")
            if name == "META-INF/LICENSE" and b"SIL OPEN FONT LICENSE Version 1.1" not in content:
                raise ValueError("Bundled font licence absent")


def stage(java, worker_target, destination):
    if destination.exists():
        raise ValueError("Use a fresh staging destination")
    directory_chain(worker_target)
    directory_chain(destination.parent, allow_missing=True)
    inventory(java)
    if not (java / "bin/java.exe").is_file():
        raise ValueError("A local Windows Java runtime is required")
    release = (java / "release").read_text(encoding="utf-8")
    version = re.search(r'^JAVA_VERSION="(21\.[^"]+)"$', release, re.M)
    if not version:
        raise ValueError("Java 21 release metadata required")
    ordinary(worker_target / "workers-0.1.0.jar")
    source_libraries = sorted((worker_target / "lib").glob("*.jar"))
    for file in source_libraries:
        ordinary(file)
    verify_pdfbox_assets(worker_target / "lib/pdfbox-3.0.8.jar")
    with zipfile.ZipFile(worker_target / "workers-0.1.0.jar") as source:
        names = source.namelist()
        if len(names) != len(set(names)):
            raise ValueError("Duplicate worker JAR entry")
        for role, worker in [("parser", "ParseWorker"), ("search", "SearchWorker")]:
            target = destination / role
            (target / "lib").mkdir(parents=True)
            shutil.copytree(java, target / "java", symlinks=False)
            classes = [n for n in names if n in {
                "workbench/Protocol.class", "workbench/FileWorker.class", f"workbench/{worker}.class",
                "workbench/WindowsJavaProbe.class", "workbench/WindowsJavaProbe$Attempt.class",
            } or (n.startswith(f"workbench/{worker}$") and n.endswith(".class"))
                or (role == "parser" and n in {"workbench/AppLocalFonts.class", "workbench/AppLocalFonts$AssetException.class"})]
            if not {"workbench/Protocol.class", "workbench/FileWorker.class", f"workbench/{worker}.class"}.issubset(classes):
                raise ValueError("Required fixed worker classes absent")
            if role == "parser" and not {"workbench/AppLocalFonts.class", "workbench/AppLocalFonts$AssetException.class"}.issubset(classes):
                raise ValueError("Fixed app-local font adapter absent")
            with zipfile.ZipFile(target / "worker.jar", "w", compression=zipfile.ZIP_DEFLATED) as output:
                for name in sorted(classes):
                    member = source.getinfo(name)
                    if member.file_size > 1024 * 1024:
                        raise ValueError("Worker class exceeds declared bound")
                    with source.open(member) as stream:
                        data = stream.read(1024 * 1024 + 1)
                    if len(data) > 1024 * 1024 or len(data) != member.file_size:
                        raise ValueError("Worker class exceeds bound or declared size")
                    output.writestr(name, data)
            selected = [p for p in source_libraries if (
                not p.name.startswith("lucene-") if role == "parser"
                else p.name.startswith(("lucene-", "jackson-"))
            )]
            if not REQUIRED[role].issubset({p.name for p in selected}):
                raise ValueError("Required pinned worker dependencies absent")
            for path in selected:
                shutil.copyfile(path, target / "lib" / path.name)
            files, directories = inventory(target)
            manifest = {"schema_version": 1, "development_only": True, "role": role,
                        "java_version": version[1], "files": files, "directories": directories}
            (target / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (destination / "development-only.json").write_text(json.dumps({"schema_version": 1, "development_only": True, "roles": ["parser", "search"]}) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java", type=Path, required=True)
    parser.add_argument("--worker-target", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    try:
        stage(args.java, args.worker_target, args.destination)
    except (OSError, ValueError, zipfile.BadZipFile, KeyError):
        parser.exit(1, "Windows development runtime staging failed; no ready bundle claimed.\n")
    print("Staged distinct parser/search development runtimes with complete inventories")


if __name__ == "__main__":
    main()
