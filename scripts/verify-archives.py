"""Verify release archives and generate the combined SHA256SUMS manifest."""
from __future__ import annotations

import argparse
import hashlib
import re
import runpy
import tarfile
import zipfile
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parent.parent
PACKAGE = runpy.run_path(str(ROOT / "scripts/package-release.py"))
POLICY = runpy.run_path(str(ROOT / "scripts/check-repository.py"))


def read_archive(file: Path, prefix: str) -> dict[str, bytes]:
    contents: dict[str, bytes] = {}

    def add(name: str, size: int, mode: int, regular: bool, reader) -> None:
        path = PurePosixPath(name)
        if path.is_absolute() or any(part in (".", "..", "") for part in name.split("/")) or "\\" in name or ":" in name or not name.startswith(prefix + "/") or not regular:
            raise RuntimeError("Unsafe or unexpected archive entry")
        relative = name[len(prefix) + 1:]
        if relative in contents:
            raise RuntimeError("Duplicate archive entry")
        if mode != PACKAGE["file_mode"](relative):
            raise RuntimeError("Unexpected archive mode")
        if size > 64 * 1024 * 1024:
            raise RuntimeError("Unexpectedly large release file")
        content = reader()
        if len(content) != size:
            raise RuntimeError("Archive file length differs")
        contents[relative] = content

    if file.suffix == ".zip":
        with zipfile.ZipFile(file) as archive:
            for member in archive.infolist():
                attributes = member.external_attr >> 16
                add(member.filename, member.file_size, attributes & 0o7777,
                    attributes & 0o170000 == 0o100000 and not member.flag_bits & 1,
                    lambda entry=member: archive.read(entry))
    else:
        with tarfile.open(file, "r:gz") as archive:
            for member in archive:
                if member.uid != 0 or member.gid != 0 or member.uname != "root" or member.gname != "root":
                    raise RuntimeError("Unexpected archive ownership")
                add(member.name, member.size, member.mode, member.isfile(),
                    lambda entry=member: archive.extractfile(entry).read())
    if not contents:
        raise RuntimeError("Archive is empty")
    return contents


def inspect_archive(file: Path, prefix: str, target: str | None) -> None:
    contents = read_archive(file, prefix)
    for name, content in contents.items():
        POLICY["check_private_content"](name, content)
        if not target and name != "SHA256SUMS":
            POLICY["check_source_name"](name)
    checksums = {}
    for line in contents.get("SHA256SUMS", b"").decode().splitlines():
        if not re.fullmatch(r"[0-9a-f]{64}  .+", line):
            raise RuntimeError("Invalid archive checksum line")
        checksum, name = line.split("  ", 1)
        if name in checksums:
            raise RuntimeError("Duplicate archive checksum entry")
        checksums[name] = checksum
    if set(checksums) != set(contents) - {"SHA256SUMS"}:
        raise RuntimeError("Archive checksum manifest is incomplete")
    for name, checksum in checksums.items():
        if hashlib.sha256(contents[name]).hexdigest() != checksum:
            raise RuntimeError(f"Archive checksum mismatch: {name}")
    required = {"README.md", "LICENSE", "THIRD_PARTY_NOTICES.md", "config.example.toml", "docs/standalone.md"}
    if target:
        info = PACKAGE["TARGETS"][target]
        name = PACKAGE["binary_name"](target)
        required.add(name)
        PACKAGE["validate_binary"](contents.get(name, b""), target)
        if info["platform"] == "linux":
            required.update({"mihomo-web-helper", "docs/deployment-guide.md", "scripts/install.sh", "scripts/uninstall.sh", "deploy/mihomo-web.service", "deploy/mihomo-web-helper.service", "deploy/mihomo-web-helper.socket"})
            PACKAGE["validate_binary"](contents.get("mihomo-web-helper", b""), target)
        else:
            if any(name.startswith(("bin/", "deploy/")) or "mihomo-web-helper" in name for name in contents):
                raise RuntimeError("Desktop archive contains Linux service programs")
            if info["platform"] == "macos":
                required.add("Start.command")
    else:
        required.update({"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml", "src/main.rs", "src/runtime.rs", "src/bin/helper.rs", "migrations/0001_initial.sql", "web/package.json", "web/package-lock.json", "web/public/favicon.svg", "web/src/App.vue", "tests/systemd_smoke.py", "tests/standalone_smoke.py", ".github/workflows/release.yml"})
    if not required.issubset(contents):
        raise RuntimeError("Archive is missing essential source or deployment files")
    print(f"Verified {file.name}: {len(checksums)} payload files")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--architecture", action="append", choices=["x86_64", "aarch64"])
    parser.add_argument("--target", action="append", choices=PACKAGE["TARGETS"])
    parser.add_argument("--require-source", action="store_true")
    parser.add_argument("--source-only", action="store_true")
    parser.add_argument("--complete", action="store_true")
    arguments = parser.parse_args()
    version = PACKAGE["version_from_project"]()
    targets = list(PACKAGE["TARGETS"]) if arguments.complete else list(dict.fromkeys((arguments.target or []) + [f"{arch}-unknown-linux-musl" for arch in arguments.architecture or []]))
    if arguments.source_only and targets:
        parser.error("--source-only cannot be combined with architecture selection")
    paths = []
    for target in targets:
        prefix = PACKAGE["release_prefix"](version, target)
        file = ROOT / "dist" / PACKAGE["release_name"](version, target)
        inspect_archive(file, prefix, target)
        paths.append(file)
    if arguments.require_source or arguments.source_only or arguments.complete:
        prefix = f"mihomo-web-panel-{version}-source"
        file = ROOT / "dist" / (prefix + ".tar.gz")
        inspect_archive(file, prefix, None)
        paths.append(file)
    if not paths:
        parser.error("Select at least one archive to verify")
    if arguments.complete and (set((ROOT / "dist").glob("*.tar.gz")) | set((ROOT / "dist").glob("*.zip"))) != set(paths):
        raise RuntimeError("Unexpected archive in the release output directory")
    manifest = "".join(f"{hashlib.sha256(file.read_bytes()).hexdigest()}  {file.name}\n" for file in sorted(paths))
    (ROOT / "dist/SHA256SUMS").write_text(manifest, encoding="utf-8")
    print("Wrote archive-level SHA256SUMS")


if __name__ == "__main__":
    main()
