"""Release invariants tested without downloading dependencies or starting services."""
from __future__ import annotations

import hashlib
import io
import runpy
import struct
import tarfile
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PACKAGE = runpy.run_path(str(ROOT / "scripts/package-release.py"))
VERIFY = runpy.run_path(str(ROOT / "scripts/verify-archives.py"))
POLICY = runpy.run_path(str(ROOT / "scripts/check-repository.py"))


def elf(machine: int = 62, segment: int = 1) -> bytes:
    # Standard ELF64 header plus a single program header, sufficient to test
    # architecture and PT_INTERP validation independently of Rust compilation.
    data = bytearray(120)
    data[:16] = b"\x7fELF\x02\x01\x01" + b"\x00" * 9
    struct.pack_into("<HHIQQQIHHHHHH", data, 16, 2, machine, 1, 0, 64, 0, 0, 64, 56, 1, 0, 0, 0)
    struct.pack_into("<I", data, 64, segment)
    return bytes(data)


def deployment_files(folder: Path) -> dict[str, Path]:
    names = {
        "README.md", "LICENSE", "THIRD_PARTY_NOTICES.md", "docs/deployment-guide.md",
        "scripts/install.sh", "scripts/uninstall.sh", "deploy/mihomo-web.service",
        "deploy/mihomo-web-helper.service", "deploy/mihomo-web-helper.socket",
        "bin/mihomo-web-panel", "bin/mihomo-web-helper",
    }
    result = {}
    for name in names:
        file = folder / name
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_bytes(elf() if name.startswith("bin/") else ("public fixture: " + name).encode())
        result[name] = file
    return result


class ReleaseTests(unittest.TestCase):
    def test_tag_must_match_version_exactly(self):
        PACKAGE["validate_tag"]("v1.2.3", "1.2.3")
        PACKAGE["validate_tag"]("v1.2.3-rc.1", "1.2.3-rc.1")
        for tag in ("main", "1.2.3", "v1.2.4", "v1.2.3/extra"):
            with self.assertRaises(RuntimeError):
                PACKAGE["validate_tag"](tag, "1.2.3")

    def test_both_static_architectures_are_recognized(self):
        PACKAGE["validate_elf"](elf(62), 62)
        PACKAGE["validate_elf"](elf(183), 183)
        with self.assertRaises(RuntimeError):
            PACKAGE["validate_elf"](elf(183), 62)

    def test_external_interpreter_is_rejected(self):
        with self.assertRaises(RuntimeError):
            PACKAGE["validate_elf"](elf(segment=3), 62)

    def test_truncated_or_non_elf_data_is_rejected(self):
        for data in (b"MZ", elf()[:64], b"\x00" * 120):
            with self.assertRaises(RuntimeError):
                PACKAGE["validate_elf"](data, 62)

    def test_archives_are_reproducible_with_correct_permissions_and_checksums(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            sources = deployment_files(folder / "inputs")
            first, second = folder / "first.tar.gz", folder / "second.tar.gz"
            for file in (first, second):
                PACKAGE["archive"](file, "release", sources, 1234567890)
                VERIFY["inspect_archive"](file, "release", "x86_64")
            self.assertEqual(first.read_bytes(), second.read_bytes())
            with tarfile.open(first, "r:gz") as package:
                manifest = package.extractfile("release/SHA256SUMS").read().decode()
                self.assertIn(hashlib.sha256(elf()).hexdigest() + "  bin/mihomo-web-panel", manifest)
                self.assertEqual(package.getmember("release/bin/mihomo-web-panel").mode, 0o755)
                self.assertEqual(package.getmember("release/README.md").mode, 0o644)
                self.assertTrue(all(member.uid == 0 and member.gid == 0 for member in package))

    def test_tampered_file_fails_checksum_verification(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            original, modified = folder / "original.tar.gz", folder / "modified.tar.gz"
            PACKAGE["archive"](original, "release", deployment_files(folder / "inputs"), 1)
            with tarfile.open(original, "r:gz") as source, tarfile.open(modified, "w:gz") as target:
                for member in source:
                    data = source.extractfile(member).read()
                    if member.name == "release/README.md":
                        data = b"tampered payload"
                        member.size = len(data)
                    target.addfile(member, io.BytesIO(data))
            with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
                VERIFY["inspect_archive"](modified, "release", "x86_64")

    def test_path_traversal_and_symlink_entries_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            for index, name in enumerate(("release/../outside", "/release/file", "other/file")):
                file = Path(temporary) / f"unsafe-{index}.tar.gz"
                with tarfile.open(file, "w:gz") as package:
                    entry = tarfile.TarInfo(name)
                    entry.type = tarfile.SYMTYPE
                    entry.linkname = "/outside"
                    package.addfile(entry)
                with self.assertRaises(RuntimeError):
                    VERIFY["inspect_archive"](file, "release", None)

    def test_source_policy_rejects_runtime_data(self):
        for name in (".tools/cache.txt", "target/app", "web/node_modules/package.json", "data/app.db", "config.toml", "bootstrap-token", "certificate.key"):
            with self.assertRaises(RuntimeError):
                POLICY["check_source_name"](name)
        POLICY["check_source_name"]("tests/support/mod.rs")

    def test_private_paths_and_credentials_are_rejected_without_leaking_values(self):
        fixtures = [
            ("C" + ":" + chr(92) + "Users" + chr(92) + "example").encode(),
            ("/" + "home" + "/example/project").encode(),
            ("ghp" + "_" + "a" * 30).encode(),
            ("-----BEGIN " + "PRIVATE KEY-----").encode(),
        ]
        for value in fixtures:
            with self.assertRaisesRegex(RuntimeError, "test.txt") as error:
                POLICY["check_private_content"]("test.txt", value)
            self.assertNotIn(value.decode(), str(error.exception))
        POLICY["check_private_content"]("fixture.txt", b"mock-controller-secret-for-tests")


if __name__ == "__main__":
    unittest.main()
