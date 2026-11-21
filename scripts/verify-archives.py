"""Verify release archives and generate the combined SHA256SUMS manifest."""
from __future__ import annotations

import argparse
import hashlib
import re
import runpy
import tarfile
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parent.parent
PACKAGE = runpy.run_path(str(ROOT / "scripts/package-release.py"))
POLICY = runpy.run_path(str(ROOT / "scripts/check-repository.py"))


def inspect_archive(file: Path, prefix: str, architecture: str | None) -> None:
    with tarfile.open(file, "r:gz") as archive:
        members = archive.getmembers()
        if not members or len({member.name for member in members}) != len(members):
            raise RuntimeError("Archive is empty or contains duplicate entries")
        contents = {}
        for member in members:
            name = PurePosixPath(member.name)
            if name.is_absolute() or ".." in name.parts or not member.name.startswith(prefix + "/") or not member.isfile():
                raise RuntimeError("Unsafe or unexpected archive entry")
            relative = member.name[len(prefix) + 1:]
            if member.uid != 0 or member.gid != 0 or member.mode != (0o755 if relative.startswith("bin/") or relative.endswith(".sh") else 0o644):
                raise RuntimeError("Unexpected archive ownership or mode")
            if member.size > 64 * 1024 * 1024:
                raise RuntimeError("Unexpectedly large release file")
            content = archive.extractfile(member).read()
            POLICY["check_private_content"](relative, content)
            if not architecture and relative != "SHA256SUMS":
                POLICY["check_source_name"](relative)
            contents[relative] = content
        manifest = contents.get("SHA256SUMS", b"").decode()
        checksums = {}
        for line in manifest.splitlines():
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
        required = {"README.md", "LICENSE", "THIRD_PARTY_NOTICES.md", "docs/deployment-guide.md"}
        if architecture:
            machine = {"x86_64": 62, "aarch64": 183}[architecture]
            for binary in ("mihomo-web-panel", "mihomo-web-helper"):
                required.add("bin/" + binary)
                PACKAGE["validate_elf"](contents.get("bin/" + binary, b""), machine)
            required.update({"scripts/install.sh", "scripts/uninstall.sh", "deploy/mihomo-web.service", "deploy/mihomo-web-helper.service", "deploy/mihomo-web-helper.socket"})
        else:
            required.update({"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "src/main.rs", "src/bin/helper.rs", "migrations/0001_initial.sql", "web/package.json", "web/package-lock.json", "web/public/favicon.svg", "web/src/App.vue", "tests/systemd_smoke.py", ".github/workflows/release.yml"})
        if not required.issubset(contents):
            raise RuntimeError("Archive is missing essential source or deployment files")
    print(f"Verified {file.name}: {len(checksums)} payload files")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--architecture", action="append", choices=["x86_64", "aarch64"])
    parser.add_argument("--require-source", action="store_true")
    parser.add_argument("--source-only", action="store_true")
    parser.add_argument("--complete", action="store_true")
    arguments = parser.parse_args()
    version = PACKAGE["version_from_project"]()
    architectures = ["x86_64", "aarch64"] if arguments.complete else arguments.architecture or []
    if arguments.source_only and architectures:
        parser.error("--source-only cannot be combined with architecture selection")
    paths = []
    for architecture in architectures:
        prefix = f"mihomo-web-panel-{version}-linux-{architecture}"
        file = ROOT / "dist" / (prefix + ".tar.gz")
        inspect_archive(file, prefix, architecture)
        paths.append(file)
    if arguments.require_source or arguments.source_only or arguments.complete:
        prefix = f"mihomo-web-panel-{version}-source"
        file = ROOT / "dist" / (prefix + ".tar.gz")
        inspect_archive(file, prefix, None)
        paths.append(file)
    if not paths:
        parser.error("Select at least one archive to verify")
    if arguments.complete and set((ROOT / "dist").glob("*.tar.gz")) != set(paths):
        raise RuntimeError("Unexpected archive in the release output directory")
    manifest = "".join(f"{hashlib.sha256(file.read_bytes()).hexdigest()}  {file.name}\n" for file in sorted(paths))
    (ROOT / "dist/SHA256SUMS").write_text(manifest, encoding="utf-8")
    print("Wrote archive-level SHA256SUMS")


if __name__ == "__main__":
    main()
