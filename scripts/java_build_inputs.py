"""Bounded POSIX input identities for the offline development Java producer.

No dependency resolution, downloads, compiler execution or staging writes here.
"""
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import zipfile


class InvalidBuild(ValueError):
    pass


def require(value, reason):
    if not value:
        raise InvalidBuild(reason)


def identity(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def signature(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_nlink, info.st_size,
            info.st_mtime_ns, info.st_ctime_ns)


def ordinary_root(root):
    require(os.name == "posix", "posix_input_reader_required")
    require(root.is_absolute() and root.resolve(strict=True) == root, "input_path_alias")
    for ancestor in (root, *root.parents):
        require(stat.S_ISDIR(ancestor.lstat().st_mode), "input_directory_link_or_special")


def file_bytes(path, maximum=256 * 1024 * 1024):
    ordinary_root(path.parent)
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1
            and 0 <= before.st_size <= maximum, "unsafe_or_oversized_input")
    fd = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        require(signature(before) == signature(os.fstat(fd)), "input_replaced")
        parts, count = [], 0
        while block := os.read(fd, min(64 * 1024, maximum + 1 - count)):
            count += len(block)
            require(count <= maximum, "growing_input")
            parts.append(block)
        require(count == before.st_size and signature(before) == signature(os.fstat(fd))
                == signature(path.lstat()), "input_changed")
        return b"".join(parts)
    finally:
        os.close(fd)


def scan(root, *, maximum_files=4096, maximum_bytes=512 * 1024 * 1024, maximum_depth=20):
    ordinary_root(root)
    rows, entries, total = [], 0, 0
    pending = [(root, 0)]
    while pending:
        directory, depth = pending.pop()
        require(depth <= maximum_depth, "input_depth_bound")
        before = directory.lstat()
        require(stat.S_ISDIR(before.st_mode), "input_directory_changed")
        with os.scandir(directory) as children:
            names = []
            for child in children:
                entries += 1
                require(entries <= maximum_files * 2, "input_entry_bound")
                names.append(child.name)
        for name in sorted(names):
            require(name not in (".", "..") and len(name.encode()) <= 255
                    and not any(ord(c) < 32 for c in name), "input_name_invalid")
            path = directory / name
            info = path.lstat()
            if stat.S_ISDIR(info.st_mode):
                pending.append((path, depth + 1))
            else:
                require(len(rows) < maximum_files and total + info.st_size <= maximum_bytes,
                        "input_aggregate_bound")
                data = file_bytes(path)
                total += len(data)
                require(total <= maximum_bytes, "input_aggregate_bound")
                rows.append({"path": path.relative_to(root).as_posix(), **identity(data),
                             "executable": bool(info.st_mode & 0o111)})
        require(signature(before) == signature(directory.lstat()), "input_directory_changed")
    rows.sort(key=lambda row: row["path"])
    return rows


def summary(rows):
    encoded = json.dumps(rows, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode()
    return {"files": len(rows), "bytes": sum(row["bytes"] for row in rows),
            "sha256": hashlib.sha256(encoded).hexdigest()}


def copy_verified(source, destination, rows):
    destination.mkdir(mode=0o700)  # Never adopt a previous build/cache.
    for row in rows:
        relative = Path(row["path"])
        require(not relative.is_absolute() and ".." not in relative.parts, "copy_path_invalid")
        data = file_bytes(source / relative)
        require(identity(data) == {k: row[k] for k in ("bytes", "sha256")}, "copy_input_drift")
        path = destination / relative
        path.parent.mkdir(parents=True, mode=0o700, exist_ok=True)
        with path.open("xb") as output:
            os.fchmod(output.fileno(), 0o700 if row["executable"] else 0o600)
            output.write(data)
    require(scan(destination) == rows, "copied_inputs_differ")


def jar_inventory(path, expected_timestamp=None):
    data = file_bytes(path, 16 * 1024 * 1024)
    rows, names, total = [], set(), 0
    with zipfile.ZipFile(io.BytesIO(data)) as jar:
        require(len(jar.infolist()) <= 256, "jar_entry_bound")
        for entry in jar.infolist():
            require(expected_timestamp is None or entry.date_time == expected_timestamp,
                    "jar_timestamp_policy_not_applied")
            name = entry.filename
            parts = name.rstrip("/").split("/")
            require(name not in names and not name.startswith("/") and "\\" not in name
                    and all(p and p not in (".", "..") for p in parts)
                    and len(name) <= 512 and not entry.flag_bits & 1,
                    "unsafe_jar_entry")
            names.add(name)
            if entry.is_dir():
                continue
            require(entry.file_size <= 2 * 1024 * 1024 and total + entry.file_size <= 16 * 1024 * 1024,
                    "jar_expansion_bound")
            with jar.open(entry) as stream:
                body = stream.read(entry.file_size + 1)
            require(len(body) == entry.file_size, "jar_member_size")
            total += len(body)
            if name.endswith(".class"):
                require(len(body) >= 8 and body[:4] == b"\xca\xfe\xba\xbe"
                        and int.from_bytes(body[6:8], "big") == 65, "class_is_not_java21_target")
            rows.append({"path": name, **identity(body)})
    require(identity(file_bytes(path, 16 * 1024 * 1024)) == identity(data), "jar_changed")
    return {**identity(data), "members": sorted(rows, key=lambda row: row["path"])}


def save(path, value):
    data = (json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n").encode()
    require(len(data) <= 2 * 1024 * 1024, "receipt_byte_bound")
    temporary = path.with_name(path.name + ".pending")
    with temporary.open("xb") as output:
        os.fchmod(output.fileno(), 0o600)
        output.write(data)
        output.flush()
        os.fsync(output.fileno())
    temporary.replace(path)
