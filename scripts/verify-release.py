"""Verify directly downloadable executables and their SHA256SUMS manifest."""
from __future__ import annotations

import argparse
import hashlib
import re
import runpy
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RELEASE = runpy.run_path(str(ROOT / "scripts/prepare-release.py"))
POLICY = runpy.run_path(str(ROOT / "scripts/check-repository.py"))


def inspect_binary(file: Path, target: str) -> bytes:
    if file.is_symlink() or not file.is_file() or not 0 < file.stat().st_size <= 64 * 1024 * 1024:
        raise RuntimeError("Expected a regular release executable of a reasonable size")
    content = file.read_bytes()
    RELEASE["validate_binary"](content, target)
    POLICY["check_private_content"](file.name, content)
    print(f"Verified executable {file.name}: {len(content):,} bytes")
    return content


def parse_checksums(content: bytes, expected: set[str]) -> dict[str, str]:
    checksums: dict[str, str] = {}
    for line in content.decode("ascii").splitlines():
        if not re.fullmatch(r"[0-9a-f]{64}  [A-Za-z0-9_.-]+", line):
            raise RuntimeError("Invalid SHA256SUMS entry")
        checksum, name = line.split("  ", 1)
        if name in checksums:
            raise RuntimeError("Duplicate SHA256SUMS entry")
        checksums[name] = checksum
    if set(checksums) != expected:
        raise RuntimeError("SHA256SUMS must cover exactly the selected release executables")
    return checksums


def verify_release(directory: Path, version: str, targets: list[str], *, complete: bool = False, write_checksums: bool = False) -> None:
    if complete and set(targets) != set(RELEASE["TARGETS"]):
        raise RuntimeError("A complete release requires all five native targets")
    if directory.is_symlink() or not directory.is_dir():
        raise RuntimeError("Release directory must be a regular directory")
    files = {RELEASE["release_name"](version, target): target for target in targets}
    if complete and {file.name for file in directory.iterdir()} - {"SHA256SUMS"} != set(files):
        raise RuntimeError("Release output must contain exactly the five platform executables and SHA256SUMS")
    contents = {name: inspect_binary(directory / name, target) for name, target in files.items()}
    manifest = directory / "SHA256SUMS"
    if manifest.is_symlink():
        raise RuntimeError("SHA256SUMS must not be a symlink")
    if write_checksums:
        manifest.write_bytes("".join(f"{hashlib.sha256(data).hexdigest()}  {name}\n" for name, data in sorted(contents.items())).encode("ascii"))
        print("Wrote executable-level SHA256SUMS")
    checksums = parse_checksums(manifest.read_bytes(), set(files))
    for name, checksum in checksums.items():
        if hashlib.sha256(contents[name]).hexdigest() != checksum:
            raise RuntimeError(f"Release checksum mismatch: {name}")
    print(f"Verified SHA256SUMS for {len(files)} executable(s)")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    selection = parser.add_mutually_exclusive_group(required=True)
    selection.add_argument("--target", action="append", choices=RELEASE["TARGETS"])
    selection.add_argument("--complete", action="store_true")
    parser.add_argument("--directory", type=Path, default=ROOT / "dist")
    parser.add_argument("--write-checksums", action="store_true", help="Generate a manifest after verifying the selected executables")
    arguments = parser.parse_args()
    targets = list(RELEASE["TARGETS"]) if arguments.complete else list(dict.fromkeys(arguments.target))
    verify_release(arguments.directory, RELEASE["version_from_project"](), targets,
                   complete=arguments.complete, write_checksums=arguments.write_checksums)


if __name__ == "__main__":
    main()
