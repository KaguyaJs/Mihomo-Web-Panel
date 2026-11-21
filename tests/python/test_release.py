"""Release invariants tested without downloading dependencies or starting services."""
from __future__ import annotations

import hashlib
import io
import runpy
import struct
import tarfile
import tempfile
import unittest
import zipfile
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
        "README.md", "LICENSE", "THIRD_PARTY_NOTICES.md", "docs/deployment-guide.md", "docs/standalone.md", "config.example.toml",
        "scripts/install.sh", "scripts/uninstall.sh", "deploy/mihomo-web.service",
        "deploy/mihomo-web-helper.service", "deploy/mihomo-web-helper.socket",
        "mihomo-web-panel", "mihomo-web-helper",
    }
    result = {}
    for name in names:
        file = folder / name
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_bytes(elf() if name in ("mihomo-web-panel", "mihomo-web-helper") else ("public fixture: " + name).encode())
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
                VERIFY["inspect_archive"](file, "release", "x86_64-unknown-linux-musl")
            self.assertEqual(first.read_bytes(), second.read_bytes())
            with tarfile.open(first, "r:gz") as package:
                manifest = package.extractfile("release/SHA256SUMS").read().decode()
                self.assertIn(hashlib.sha256(elf()).hexdigest() + "  mihomo-web-panel", manifest)
                self.assertEqual(package.getmember("release/mihomo-web-panel").mode, 0o755)
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
                VERIFY["inspect_archive"](modified, "release", "x86_64-unknown-linux-musl")

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
        for name in (".tools/cache.txt", "target/app", "web/node_modules/package.json", "data/app.db", "config.toml", "nested/config.toml", "config.lock", "instance.lock", "bootstrap-token", "certificate.key"):
            with self.assertRaises(RuntimeError):
                POLICY["check_source_name"](name)
        POLICY["check_source_name"]("tests/support/mod.rs")
        POLICY["check_source_name"](".cargo/config.toml")

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

    def test_binary_metadata_is_scanned_for_private_paths(self):
        private = ("D" + ":" + chr(92) + "Users" + chr(92) + "example" + chr(92) + "source.rs").encode()
        with self.assertRaises(RuntimeError):
            POLICY["check_private_content"]("binary", b"\xff\0" + private + b"\0\xff")

    def test_macos_header_and_system_library_policy(self):
        def macho(machine=0x0100000C, library=b"/usr/lib/libSystem.B.dylib"):
            command = bytearray(24 + len(library) + 1)
            struct.pack_into("<III", command, 0, 0xC, len(command), 24)
            command[24:24 + len(library)] = library
            header = struct.pack("<IIIIIIII", 0xFEEDFACF, machine, 0, 2, 1, len(command), 0, 0)
            return header + command
        PACKAGE["validate_macho"](macho(), 0x0100000C)
        PACKAGE["validate_macho"](macho(0x01000007), 0x01000007)
        for data in (macho()[:32], macho(library=b"@rpath/private.dylib"), macho(0x01000007)):
            with self.assertRaises(RuntimeError):
                PACKAGE["validate_macho"](data, 0x0100000C)

    def test_windows_pe_requires_correct_architecture_and_no_external_runtime(self):
        def pe(library=b"KERNEL32.dll", machine=0x8664):
            data = bytearray(1024)
            data[:2] = b"MZ"
            struct.pack_into("<I", data, 60, 64)
            data[64:68] = b"PE\0\0"
            struct.pack_into("<HH", data, 68, machine, 1)
            struct.pack_into("<HH", data, 84, 240, 2)
            struct.pack_into("<H", data, 88, 0x20B)
            struct.pack_into("<II", data, 208, 0x1000, 40)
            struct.pack_into("<IIII", data, 336, 512, 0x1000, 512, 512)
            struct.pack_into("<IIIII", data, 512, 1, 0, 0, 0x1050, 1)
            data[592:592 + len(library) + 1] = library + b"\0"
            return bytes(data)
        PACKAGE["validate_pe"](pe(), 0x8664)
        for data in (pe(b"VCRUNTIME140.dll"), pe(b"ucrtbase.dll"), pe(machine=0xAA64), pe()[:400]):
            with self.assertRaises(RuntimeError):
                PACKAGE["validate_pe"](data, 0x8664)

    def test_zip_reproducibility_modes_and_unsafe_entries(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            inputs = deployment_files(folder / "inputs")
            first, second = folder / "first.zip", folder / "second.zip"
            for output in (first, second):
                PACKAGE["archive"](output, "release", inputs, 1234567890)
            self.assertEqual(first.read_bytes(), second.read_bytes())
            self.assertEqual(VERIFY["read_archive"](first, "release")["mihomo-web-panel"], elf())
            with zipfile.ZipFile(first) as package:
                self.assertEqual(package.getinfo("release/mihomo-web-panel").external_attr >> 16 & 0o777, 0o755)
            unsafe = folder / "unsafe.zip"
            with zipfile.ZipFile(unsafe, "w") as package:
                entry = zipfile.ZipInfo("release/../file")
                entry.external_attr = 0o100644 << 16
                package.writestr(entry, "unsafe")
            with self.assertRaises(RuntimeError):
                VERIFY["read_archive"](unsafe, "release")

    def test_all_platform_archives_have_unique_names(self):
        names = {PACKAGE["release_name"]("1.2.3", target) for target in PACKAGE["TARGETS"]}
        self.assertEqual(len(names), 5)
        self.assertIn("mihomo-web-panel-1.2.3-windows-x86_64.zip", names)
        self.assertIn("mihomo-web-panel-1.2.3-macos-aarch64.tar.gz", names)


if __name__ == "__main__":
    unittest.main()
