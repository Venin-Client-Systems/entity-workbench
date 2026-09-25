"""Read-only, bounded inventory of explicitly selected development Java/Search assets.

No Java invocation, downloader, runtime discovery or release packaging claim.
The canonical digest is SHA256 of compact ASCII JSON rows [path,bytes,sha256,executable].
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat


class InvalidRuntime(ValueError):
    pass


def require(ok, reason):
    if not ok:
        raise InvalidRuntime(reason)


def signature(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_nlink,
            info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def canonical_digest(rows):
    return hashlib.sha256(json.dumps(rows, separators=(",", ":"), ensure_ascii=True).encode()).hexdigest()


def ordinary_root(path):
    require(os.name == "posix", "posix_inventory_required")
    require(path.is_absolute() and path.resolve(strict=True) == path, "runtime_path_alias")
    for ancestor in (path, *path.parents):
        require(stat.S_ISDIR(ancestor.lstat().st_mode), "nonordinary_directory_ancestor")


def inventory(root):
    root = Path(root)
    ordinary_root(root)
    rows, counts = [], [2, 0]
    flags = os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK

    def walk(path, fd, relative, depth):
        require(depth <= 8, "runtime_depth_exceeded")
        before = os.fstat(fd)
        require(stat.S_ISDIR(before.st_mode) and signature(before) == signature(path.lstat()),
                "runtime_directory_replaced")
        names = []
        with os.scandir(fd) as entries:
            for entry in entries:
                counts[0] += 1
                require(counts[0] <= 1024, "runtime_entries_exceeded")
                names.append(entry.name)
        for name in sorted(names):
            require(re.fullmatch(r"[A-Za-z0-9._+\-]{1,180}", name) is not None and name not in (".", ".."),
                    "invalid_runtime_member_name")
            info = os.stat(name, dir_fd=fd, follow_symlinks=False)
            directory = stat.S_ISDIR(info.st_mode)
            require(directory or (stat.S_ISREG(info.st_mode) and info.st_nlink == 1),
                    "runtime_link_or_special_entry")
            child = os.open(name, flags | (os.O_DIRECTORY if directory else 0), dir_fd=fd)
            try:
                require(signature(info) == signature(os.fstat(child)), "runtime_entry_replaced")
                member = relative + "/" + name
                if directory:
                    walk(path / name, child, member, depth + 1)
                else:
                    require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1
                            and info.st_size <= 128 * 1024 * 1024, "unsafe_or_oversized_runtime_file")
                    require(counts[1] + info.st_size <= 256 * 1024 * 1024, "runtime_aggregate_exceeded")
                    count, sha = 0, hashlib.sha256()
                    while block := os.read(child, 64 * 1024):
                        count += len(block)
                        require(count <= 128 * 1024 * 1024, "growing_runtime_file")
                        sha.update(block)
                    counts[1] += count
                    require(count == info.st_size and counts[1] <= 256 * 1024 * 1024,
                            "runtime_byte_bound_or_size_changed")
                    require(signature(info) == signature(os.fstat(child)), "runtime_file_changed")
                    rows.append([member, count, sha.hexdigest(), bool(info.st_mode & 0o111)])
                require(signature(info) == signature(os.stat(name, dir_fd=fd, follow_symlinks=False)),
                        "runtime_entry_changed")
            finally:
                os.close(child)
        require(signature(before) == signature(os.fstat(fd)) == signature(path.lstat()),
                "runtime_directory_changed")

    for component in ("java", "search"):
        fd = os.open(root / component, flags | os.O_DIRECTORY)
        try:
            walk(root / component, fd, component, 1)
        finally:
            os.close(fd)
    rows.sort()
    by_name = {row[0]: row for row in rows}
    for name in ("java/bin/java", "java/release", "search/workers-0.1.0.jar"):
        require(name in by_name and by_name[name][1] > 0, "incomplete_selected_runtime")
    require(by_name["java/bin/java"][3], "selected_java_not_executable")
    require(any(name.startswith("search/lib/") and name.endswith(".jar") for name in by_name),
            "search_dependency_jars_missing")
    return {"sha256": canonical_digest(rows), "files": rows, "file_count": len(rows),
            "bytes": sum(row[1] for row in rows), "claim": "pinned_selected_development_artifact",
            "rebuilt_from_current_java_source": False, "complete_release": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(inventory(args.runtime), sort_keys=True, indent=2))


if __name__ == "__main__":
    main()
