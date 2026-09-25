"""Relocate an existing macOS Tesseract installation; no downloads or release claims.

Build-machine tools only: otool, install_name_tool and codesign. Execution uses the
resulting app-local paths and never searches Homebrew or PATH. A release build must
replace ad-hoc signing with its own reviewed signing/notarization procedure.
"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
MODEL_SHA256 = "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2"


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def dependencies(path):
    names = [line.strip().split(" (")[0] for line in subprocess.check_output(
        ["/usr/bin/otool", "-L", str(path)], text=True).splitlines()[1:]]
    identity = subprocess.check_output(["/usr/bin/otool", "-D", str(path)], text=True).splitlines()[1:]
    return [name for name in names if name not in identity]


def resolve_dependency(source, name):
    if name.startswith("/"):
        return Path(name).resolve(strict=True)
    if name.startswith("@loader_path/"):
        return (source.parent / name.removeprefix("@loader_path/")).resolve(strict=True)
    if name.startswith("@rpath/"):
        commands = subprocess.check_output(["/usr/bin/otool", "-l", str(source)], text=True)
        roots = re.findall(r"cmd LC_RPATH\n\s+cmdsize \d+\n\s+path (.*?) \(offset", commands)
        candidates = {Path(root.replace("@loader_path", str(source.parent))) / name.removeprefix("@rpath/") for root in roots}
        resolved = {p.resolve(strict=True) for p in candidates if p.is_file()}
        if len(resolved) == 1:
            return resolved.pop()
    raise ValueError("Ambiguous or unsupported source dependency")


def system(path):
    return path.startswith(("/usr/lib/", "/System/Library/"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tesseract", type=Path, required=True)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--destination", type=Path, default=ROOT / "runtime/staged/engines/ocr")
    args = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("Native macOS staging only")
    executable = args.tesseract.resolve(strict=True)
    model = args.model.resolve(strict=True)
    version = subprocess.check_output([str(executable), "--version"], text=True)
    if version.splitlines()[0] != "tesseract 5.5.2" or digest(model) != MODEL_SHA256:
        parser.error("Reviewed Tesseract 5.5.2 and tessdata_fast 4.1.0 English are required")
    target = args.destination.absolute()
    if target.exists():
        parser.error("Destination already exists; use a fresh staging directory")
    sources = {}
    pending = [executable]
    system_libraries = set()
    while pending:
        source = pending.pop()
        if source in sources:
            continue
        architectures = subprocess.check_output(["/usr/bin/lipo", "-archs", str(source)], text=True).split()
        if platform.machine() not in architectures:
            parser.error("Native architecture is missing from a runtime asset")
        deps = dependencies(source)
        sources[source] = deps
        for name in deps:
            if system(name):
                system_libraries.add(name)
            else:
                pending.append(resolve_dependency(source, name))
    destinations = {source: Path("bin/tesseract") if source == executable else Path("lib") / source.name for source in sources}
    if len(set(destinations.values())) != len(destinations):
        parser.error("Dependency basename collision")
    for name in ("bin", "lib", "tessdata", "licenses"):
        (target / name).mkdir(parents=True, exist_ok=False)
    packages = {}
    for source, relative in destinations.items():
        copied = target / relative
        shutil.copyfile(source, copied)
        copied.chmod(0o755)
        # Preserve source package/version/licences without leaking build-machine paths.
        parts = source.parts
        if "Cellar" not in parts:
            parser.error("Source package provenance unavailable; expected an installed formula")
        index = parts.index("Cellar")
        package, package_version = parts[index + 1:index + 3]
        prefix = Path(*parts[:index + 3])
        key = f"{package}-{package_version}"
        if key not in packages:
            formula = prefix / ".brew" / f"{package}.rb"
            license_files = [p for p in prefix.iterdir() if p.is_file() and any(
                word in p.name.lower() for word in ("license", "copying", "notice", "copyright"))]
            if package == "leptonica":
                license_files.append(ROOT / "third_party/ocr/leptonica-LICENSE")
            if not license_files or not formula.is_file():
                parser.error(f"License/provenance files unavailable for {key}")
            license_dir = target / "licenses" / key
            license_dir.mkdir()
            for license_file in license_files:
                shutil.copyfile(license_file, license_dir / license_file.name)
            packages[key] = {"package": package, "version": package_version,
                             "formula_sha256": digest(formula), "source_files": {}, "architectures": {}}
        packages[key]["source_files"][str(relative)] = digest(source)
        packages[key]["architectures"][str(relative)] = subprocess.check_output(["/usr/bin/lipo", "-archs", str(source)], text=True).split()
        changes = ["/usr/bin/install_name_tool"]
        if source != executable:
            changes += ["-id", f"@loader_path/{source.name}"]
        for dependency in sources[source]:
            if not system(dependency):
                local = destinations[resolve_dependency(source, dependency)]
                replacement = f"@loader_path/../lib/{local.name}" if source == executable else f"@loader_path/{local.name}"
                changes += ["-change", dependency, replacement]
        subprocess.run(changes + [str(copied)], check=True, capture_output=True)
        subprocess.run(["/usr/bin/codesign", "--force", "--sign", "-", str(copied)], check=True, capture_output=True)
    shutil.copyfile(model, target / "tessdata/eng.traineddata")
    shutil.copyfile(ROOT / "third_party/ocr/tessdata_fast-LICENSE", target / "licenses/tessdata_fast-LICENSE")
    notices = {"schema_version": 1, "development_only": True, "modifications": "Mach-O dependency paths rewritten to app-local loader paths; signatures replaced with ad-hoc development signatures.",
               "packages": packages, "system_libraries": sorted(system_libraries), "model": {"name": "tessdata_fast", "version": "4.1.0", "language": "eng", "sha256": MODEL_SHA256, "source": "https://github.com/tesseract-ocr/tessdata_fast/tree/4.1.0"}}
    (target / "NOTICE.json").write_text(json.dumps(notices, indent=2) + "\n")
    for source, relative in destinations.items():
        for name in dependencies(target / relative):
            if not system(name) and not name.startswith("@loader_path/"):
                parser.error("Non-local dependency remains after relocation")
    files = {str(p.relative_to(target)): {"bytes": p.stat().st_size, "sha256": digest(p)}
             for p in sorted(target.rglob("*")) if p.is_file()}
    manifest = {"schema_version": 1, "os": "macos", "architecture": platform.machine(), "engine": "tesseract-5.5.2", "language": "eng", "model_sha256": MODEL_SHA256, "files": files}
    (target / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    relocated = subprocess.check_output([str(target / "bin/tesseract"), "--version"], env={}, text=True)
    if relocated.splitlines()[0] != "tesseract 5.5.2":
        parser.error("Relocated runtime identity mismatch")
    print(json.dumps({"engine": manifest["engine"], "architecture": manifest["architecture"], "files": len(files), "manifest_sha256": digest(target / "manifest.json"), "complete_release": False}))


if __name__ == "__main__":
    main()
