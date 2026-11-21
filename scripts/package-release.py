"""Create Linux binary and complete source archives, with SHA256 checksums.

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
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "x86_64-unknown-linux-musl": ("x86_64", 62),
    "aarch64-unknown-linux-musl": ("aarch64", 183),
}
SHARED_FILES = ["README.md", "LICENSE", "THIRD_PARTY_NOTICES.md", "CHANGELOG.md", "SECURITY.md"]
SOURCE_FILES = SHARED_FILES + ["CONTRIBUTING.md", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "config.example.toml", ".editorconfig", ".gitattributes", ".gitignore"]
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
    with temporary.open("wb") as output, gzip.GzipFile(filename="", fileobj=output, mode="wb", mtime=timestamp, compresslevel=9) as compressed, tarfile.open(fileobj=compressed, mode="w|", format=tarfile.PAX_FORMAT) as tar:
        for name, content in contents.items():
            entry = tarfile.TarInfo(prefix + "/" + name)
            entry.size = len(content)
            entry.mtime = timestamp
            entry.mode = 0o755 if name.startswith("bin/") or name.endswith(".sh") else 0o644
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
    architecture, machine = TARGETS[arguments.target]
    version = version_from_project()
    if arguments.tag:
        validate_tag(arguments.tag, version)
    if not (ROOT / "THIRD_PARTY_NOTICES.md").is_file():
        raise RuntimeError("Complete dependency notices before packaging")
    release = f"mihomo-web-panel-{version}-linux-{architecture}"
    timestamp = build_timestamp()
    distribution = ROOT / "dist"
    distribution.mkdir(exist_ok=True)
    outputs = []
    if not arguments.source_only:
        binary_sources = {name: ROOT / name for name in SHARED_FILES}
        for binary in ("mihomo-web-panel", "mihomo-web-helper"):
            file = ROOT / "target" / arguments.target / "release" / binary
            verify_elf(file, machine)
            binary_sources["bin/" + binary] = file
        for folder in ("deploy", "docs", "third_party"):
            binary_sources.update(files_under(folder))
        for name in ("install.sh", "uninstall.sh"):
            binary_sources["scripts/" + name] = ROOT / "scripts" / name
        binary_archive = distribution / (release + ".tar.gz")
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
