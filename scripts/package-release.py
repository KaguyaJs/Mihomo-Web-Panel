"""Create standalone platform binaries and complete source archives with SHA256.

Run after frontend build, Rust release build, verification and license collection.
Python 3.11+ is sufficient; no third-party Python packages are needed.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import os
import re
import struct
import subprocess
import tarfile
import time
import tomllib
import zipfile
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "x86_64-unknown-linux-musl": {"platform": "linux", "architecture": "x86_64", "machine": 62, "format": "tar.gz", "runner": "ubuntu-24.04"},
    "aarch64-unknown-linux-musl": {"platform": "linux", "architecture": "aarch64", "machine": 183, "format": "tar.gz", "runner": "ubuntu-24.04-arm"},
    "x86_64-pc-windows-msvc": {"platform": "windows", "architecture": "x86_64", "machine": 0x8664, "format": "zip", "runner": "windows-latest"},
    "x86_64-apple-darwin": {"platform": "macos", "architecture": "x86_64", "machine": 0x01000007, "format": "tar.gz", "runner": "macos-15-intel"},
    "aarch64-apple-darwin": {"platform": "macos", "architecture": "aarch64", "machine": 0x0100000C, "format": "tar.gz", "runner": "macos-15"},
}
SHARED_FILES = ["README.md", "LICENSE", "THIRD_PARTY_NOTICES.md", "CHANGELOG.md", "SECURITY.md", "config.example.toml"]
SOURCE_FILES = SHARED_FILES + ["CONTRIBUTING.md", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml", ".editorconfig", ".gitattributes", ".gitignore"]
SOURCE_DIRECTORIES = ["src", "migrations", "tests", "examples", "scripts", "deploy", "docs", "third_party", ".github", "web/src", "web/e2e", "web/public"]
FRONTEND_FILES = ["package.json", "package-lock.json", "index.html", "vite.config.ts", "vitest.config.ts", "playwright.config.ts", "tsconfig.json", "tsconfig.app.json", "tsconfig.node.json"]


def files_under(folder: str) -> dict[str, Path]:
    result: dict[str, Path] = {}
    for file in (ROOT / folder).rglob("*"):
        if file.is_symlink():
            raise RuntimeError(f"Symlink is not allowed in release input: {file.relative_to(ROOT)}")
        if file.is_file() and "__pycache__" not in file.parts and not file.name.endswith((".pyc", ".log", ".db", ".db-wal", ".db-shm", ".key", ".pem")):
            result[file.relative_to(ROOT).as_posix()] = file
    return result


def validate_elf(data: bytes, machine: int) -> None:
    if len(data) < 64 or data[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", data, 18)[0] != machine:
        raise RuntimeError("Not the expected 64-bit little-endian Linux ELF binary")
    offset = struct.unpack_from("<Q", data, 32)[0]
    size, count = struct.unpack_from("<HH", data, 54)
    if not count or size < 56 or offset < 64 or offset + count * size > len(data):
        raise RuntimeError("Invalid ELF program header table")
    for index in range(count):
        if struct.unpack_from("<I", data, offset + index * size)[0] == 3:
            raise RuntimeError("Binary still needs an external ELF interpreter")


def verify_elf(file: Path, machine: int) -> None:
    validate_elf(file.read_bytes(), machine)


def c_string(data: bytes, offset: int, end: int) -> str:
    if not 0 <= offset < end <= len(data):
        raise RuntimeError("Invalid executable string offset")
    stop = data.find(b"\0", offset, end)
    if stop == -1:
        raise RuntimeError("Unterminated executable string")
    return data[offset:stop].decode("ascii")


def validate_pe(data: bytes, machine: int) -> None:
    if len(data) < 64 or data[:2] != b"MZ":
        raise RuntimeError("Not a Windows PE executable")
    offset = struct.unpack_from("<I", data, 60)[0]
    if offset < 64 or offset + 24 > len(data) or data[offset:offset + 4] != b"PE\0\0":
        raise RuntimeError("Invalid Windows PE header")
    actual, sections = struct.unpack_from("<HH", data, offset + 4)
    size, flags = struct.unpack_from("<HH", data, offset + 20)
    optional = offset + 24
    table = optional + size
    if actual != machine or not flags & 2 or size < 128 or table + sections * 40 > len(data):
        raise RuntimeError("Unexpected Windows architecture or header")
    if struct.unpack_from("<H", data, optional)[0] != 0x20B:
        raise RuntimeError("Windows release must use PE32+")

    def file_offset(rva: int) -> int:
        for index in range(sections):
            section = table + index * 40
            virtual_size, address, raw_size, raw = struct.unpack_from("<IIII", data, section + 8)
            if address <= rva < address + max(virtual_size, raw_size):
                position = raw + rva - address
                if not raw <= position < raw + raw_size <= len(data):
                    break
                return position
        raise RuntimeError("Invalid PE import address")

    import_rva, import_size = struct.unpack_from("<II", data, optional + 120)
    if not import_rva or import_size < 20:
        raise RuntimeError("Windows executable is missing its system import table")
    imports = file_offset(import_rva)
    terminated = False
    for entry in range(imports, min(imports + import_size, len(data)) - 19, 20):
        values = struct.unpack_from("<IIIII", data, entry)
        if not any(values):
            terminated = True
            break
        name = c_string(data, file_offset(values[3]), len(data)).lower()
        if name.startswith(("vcruntime", "msvcp", "msvcr", "api-ms-win-crt-", "libgcc", "libstdc++")) or name == "ucrtbase.dll":
            raise RuntimeError("Windows executable requires an external C/C++ runtime")
    if not terminated:
        raise RuntimeError("Unterminated PE import table")


def validate_macho(data: bytes, machine: int) -> None:
    if len(data) < 32 or data[:4] != b"\xcf\xfa\xed\xfe":
        raise RuntimeError("Not a 64-bit little-endian macOS Mach-O executable")
    actual, _, kind, count, commands_size = struct.unpack_from("<IIIII", data, 4)
    if actual != machine or kind != 2 or not count or 32 + commands_size > len(data):
        raise RuntimeError("Unexpected macOS architecture or header")
    offset = 32
    for _ in range(count):
        if offset + 8 > 32 + commands_size:
            raise RuntimeError("Truncated Mach-O command")
        command, size = struct.unpack_from("<II", data, offset)
        if size < 8 or offset + size > 32 + commands_size:
            raise RuntimeError("Invalid Mach-O command size")
        if command in (0xC, 0x80000018, 0x8000001F, 0x80000023):
            if size < 24:
                raise RuntimeError("Invalid Mach-O library command")
            name_offset = struct.unpack_from("<I", data, offset + 8)[0]
            if name_offset < 24:
                raise RuntimeError("Invalid Mach-O library name")
            library = c_string(data, offset + name_offset, offset + size)
            if not library.startswith(("/usr/lib/", "/System/Library/")):
                raise RuntimeError("macOS executable requires a non-system library")
        elif command == 0x8000001C:
            raise RuntimeError("macOS executable contains an external runtime search path")
        offset += size
    if offset != 32 + commands_size:
        raise RuntimeError("Mach-O command table length differs")


def binary_name(target: str) -> str:
    return "mihomo-web-panel.exe" if TARGETS[target]["platform"] == "windows" else "mihomo-web-panel"


def release_prefix(version: str, target: str) -> str:
    info = TARGETS[target]
    return f"mihomo-web-panel-{version}-{info['platform']}-{info['architecture']}"


def release_name(version: str, target: str) -> str:
    return release_prefix(version, target) + "." + TARGETS[target]["format"]


def validate_binary(data: bytes, target: str) -> None:
    info = TARGETS[target]
    validator = {"linux": validate_elf, "windows": validate_pe, "macos": validate_macho}[info["platform"]]
    validator(data, info["machine"])


def file_mode(name: str) -> int:
    return 0o755 if name.startswith("bin/") or name in ("mihomo-web-panel", "mihomo-web-panel.exe", "mihomo-web-helper") or name.endswith((".sh", ".command")) else 0o644


def version_from_project() -> str:
    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    if not re.fullmatch(r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?", version):
        raise RuntimeError("Package version is not a supported semantic version")
    return version


def validate_tag(tag: str, version: str) -> None:
    if tag != "v" + version:
        raise RuntimeError("Release tag must match the package version exactly")


def source_inputs() -> dict[str, Path]:
    sources = {name: ROOT / name for name in SOURCE_FILES}
    for folder in SOURCE_DIRECTORIES:
        sources.update(files_under(folder))
    for name in FRONTEND_FILES:
        file = ROOT / "web" / name
        if file.is_file():
            sources["web/" + name] = file
    if (ROOT / ".git").exists():
        tracked = set(subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).decode().split("\0"))
        sources = {name: file for name, file in sources.items() if name in tracked}
        if any(name not in sources for name in SOURCE_FILES):
            raise RuntimeError("Required source files must be tracked before packaging")
    return sources


def build_timestamp() -> int:
    if "SOURCE_DATE_EPOCH" in os.environ:
        return int(os.environ["SOURCE_DATE_EPOCH"])
    if (ROOT / ".git").exists():
        return int(subprocess.check_output(["git", "log", "-1", "--format=%ct"], cwd=ROOT))
    return int(time.time())


def archive(destination: Path, prefix: str, sources: dict[str, Path], timestamp: int) -> None:
    contents = {name: file.read_bytes() for name, file in sorted(sources.items())}
    checksum = "".join(f"{hashlib.sha256(content).hexdigest()}  {name}\n" for name, content in contents.items())
    contents["SHA256SUMS"] = checksum.encode()
    temporary = destination.with_suffix(destination.suffix + ".tmp")
    if destination.suffix == ".zip":
        moment = datetime.fromtimestamp(max(315532800, min(timestamp, 4354819198)), timezone.utc)
        date = (moment.year, moment.month, moment.day, moment.hour, moment.minute, moment.second // 2 * 2)
        with zipfile.ZipFile(temporary, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as package:
            for name, content in contents.items():
                entry = zipfile.ZipInfo(prefix + "/" + name, date)
                entry.create_system = 3
                entry.external_attr = (0o100000 | file_mode(name)) << 16
                entry.compress_type = zipfile.ZIP_DEFLATED
                package.writestr(entry, content, compresslevel=9)
    else:
        with temporary.open("wb") as output, gzip.GzipFile(filename="", fileobj=output, mode="wb", mtime=timestamp, compresslevel=9) as compressed, tarfile.open(fileobj=compressed, mode="w|", format=tarfile.PAX_FORMAT) as tar:
            for name, content in contents.items():
                entry = tarfile.TarInfo(prefix + "/" + name)
                entry.size = len(content)
                entry.mtime = timestamp
                entry.mode = file_mode(name)
                entry.uid = entry.gid = 0
                entry.uname = entry.gname = "root"
                tar.addfile(entry, io.BytesIO(content))
    temporary.replace(destination)
    print(f"Created {destination.name}: {destination.stat().st_size:,} bytes, {len(sources)} files")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=TARGETS, default="x86_64-unknown-linux-musl")
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--source-only", action="store_true")
    selection.add_argument("--binary-only", action="store_true")
    parser.add_argument("--tag", help="Require an exact vX.Y.Z version match")
    arguments = parser.parse_args()
    info = TARGETS[arguments.target]
    version = version_from_project()
    if arguments.tag:
        validate_tag(arguments.tag, version)
    if not (ROOT / "THIRD_PARTY_NOTICES.md").is_file():
        raise RuntimeError("Complete dependency notices before packaging")
    release = release_prefix(version, arguments.target)
    timestamp = build_timestamp()
    distribution = ROOT / "dist"
    distribution.mkdir(exist_ok=True)
    outputs = []
    if not arguments.source_only:
        binary_sources = {name: ROOT / name for name in SHARED_FILES}
        binaries = [binary_name(arguments.target)]
        if info["platform"] == "linux":
            binaries.append("mihomo-web-helper")
        for binary in binaries:
            file = ROOT / "target" / arguments.target / "release" / binary
            validate_binary(file.read_bytes(), arguments.target)
            binary_sources[binary] = file
        for folder in ("docs", "third_party"):
            binary_sources.update(files_under(folder))
        if info["platform"] == "linux":
            binary_sources.update(files_under("deploy"))
            for name in ("install.sh", "uninstall.sh"):
                binary_sources["scripts/" + name] = ROOT / "scripts" / name
        elif info["platform"] == "macos":
            binary_sources["Start.command"] = ROOT / "scripts/start-macos.command"
        binary_archive = distribution / release_name(version, arguments.target)
        archive(binary_archive, release, binary_sources, timestamp)
        outputs.append(binary_archive)
    if not arguments.binary_only:
        source_archive = distribution / f"mihomo-web-panel-{version}-source.tar.gz"
        archive(source_archive, f"mihomo-web-panel-{version}-source", source_inputs(), timestamp)
        outputs.append(source_archive)
    checksums = "".join(f"{hashlib.sha256(file.read_bytes()).hexdigest()}  {file.name}\n" for file in outputs)
    (distribution / "SHA256SUMS").write_text(checksums, encoding="utf-8")
    print("Each archive includes a complete SHA256SUMS manifest")


if __name__ == "__main__":
    main()
