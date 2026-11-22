"""Executable release invariants, independent of native compilation or services."""
from __future__ import annotations

import hashlib
import os
import runpy
import stat
import struct
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RELEASE = runpy.run_path(str(ROOT / "scripts/prepare-release.py"))
VERIFY = runpy.run_path(str(ROOT / "scripts/verify-release.py"))
POLICY = runpy.run_path(str(ROOT / "scripts/check-repository.py"))


def elf(machine: int = 62, segment: int = 1) -> bytes:
    # An ELF64 header and one program header exercise the loader policy without
    # executing synthetic machine code or invoking a cross-platform compiler.
    data = bytearray(120)
    data[:16] = b"\x7fELF\x02\x01\x01" + b"\x00" * 9
    struct.pack_into("<HHIQQQIHHHHHH", data, 16, 2, machine, 1, 0, 64, 0, 0, 64, 56, 1, 0, 0, 0)
    struct.pack_into("<I", data, 64, segment)
    return bytes(data)


def pe(library: bytes = b"KERNEL32.dll", machine: int = 0x8664) -> bytes:
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


def macho(machine: int = 0x0100000C, library: bytes = b"/usr/lib/libSystem.B.dylib") -> bytes:
    command = bytearray(24 + len(library) + 1)
    struct.pack_into("<III", command, 0, 0xC, len(command), 24)
    command[24:24 + len(library)] = library
    return struct.pack("<IIIIIIII", 0xFEEDFACF, machine, 0, 2, 1, len(command), 0, 0) + command


def release_files(directory: Path, version: str = "1.2.3") -> list[str]:
    directory.mkdir(parents=True, exist_ok=True)
    for target, info in RELEASE["TARGETS"].items():
        factory = {"linux": elf, "windows": lambda machine: pe(machine=machine), "macos": macho}[info["platform"]]
        (directory / RELEASE["release_name"](version, target)).write_bytes(factory(info["machine"]))
    return list(RELEASE["TARGETS"])


class ReleaseTests(unittest.TestCase):
    def test_tag_must_match_version_exactly(self):
        RELEASE["validate_tag"]("v1.2.3", "1.2.3")
        RELEASE["validate_tag"]("v1.2.3-rc.1", "1.2.3-rc.1")
        for tag in ("main", "1.2.3", "v1.2.4", "v1.2.3/extra"):
            with self.assertRaises(RuntimeError):
                RELEASE["validate_tag"](tag, "1.2.3")

    def test_both_static_linux_architectures_are_recognized(self):
        RELEASE["validate_elf"](elf(62), 62)
        RELEASE["validate_elf"](elf(183), 183)
        with self.assertRaises(RuntimeError):
            RELEASE["validate_elf"](elf(183), 62)

    def test_external_interpreter_is_rejected(self):
        with self.assertRaises(RuntimeError):
            RELEASE["validate_elf"](elf(segment=3), 62)

    def test_truncated_or_non_elf_data_is_rejected(self):
        for data in (b"MZ", elf()[:64], b"\x00" * 120):
            with self.assertRaises(RuntimeError):
                RELEASE["validate_elf"](data, 62)

    def test_windows_pe_requires_correct_architecture_and_no_external_runtime(self):
        RELEASE["validate_pe"](pe(), 0x8664)
        for data in (pe(b"VCRUNTIME140.dll"), pe(b"ucrtbase.dll"), pe(machine=0xAA64), pe()[:400]):
            with self.assertRaises(RuntimeError):
                RELEASE["validate_pe"](data, 0x8664)

    def test_macos_requires_correct_architecture_and_system_libraries(self):
        RELEASE["validate_macho"](macho(), 0x0100000C)
        RELEASE["validate_macho"](macho(0x01000007), 0x01000007)
        for data in (macho()[:32], macho(library=b"@rpath/private.dylib"), macho(0x01000007)):
            with self.assertRaises(RuntimeError):
                RELEASE["validate_macho"](data, 0x0100000C)

    def test_five_downloads_are_unique_raw_executables(self):
        names = {RELEASE["release_name"]("1.2.3", target) for target in RELEASE["TARGETS"]}
        self.assertEqual(len(names), 5)
        self.assertIn("mihomo-web-panel-1.2.3-windows-x86_64.exe", names)
        self.assertIn("mihomo-web-panel-1.2.3-macos-aarch64", names)
        self.assertTrue(all(not name.endswith((".zip", ".gz", ".tar", ".command")) for name in names))

    def test_staging_preserves_exact_executable_bytes_without_sidecar_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "compiled"
            source.write_bytes(elf())
            destination = root / "dist" / "mihomo-web-panel-1.2.3-linux-x86_64"
            RELEASE["stage_binary"](source, destination, "x86_64-unknown-linux-musl")
            self.assertEqual(source.read_bytes(), destination.read_bytes())
            self.assertEqual(list(destination.parent.iterdir()), [destination])
            if os.name != "nt":
                self.assertEqual(stat.S_IMODE(destination.stat().st_mode), 0o755)

    def test_staging_rejects_wrong_architecture_and_private_metadata(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, destination = root / "compiled", root / "dist" / "public"
            private = ("D" + ":" + chr(92) + "Users" + chr(92) + "example" + chr(92) + "source.rs").encode()
            for content in (elf(183), elf() + b"\0" + private + b"\0"):
                source.write_bytes(content)
                with self.assertRaises(RuntimeError):
                    RELEASE["stage_binary"](source, destination, "x86_64-unknown-linux-musl")
                self.assertFalse(destination.exists())

    def test_complete_release_has_exactly_five_executables_and_matching_checksums(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            targets = release_files(directory)
            VERIFY["verify_release"](directory, "1.2.3", targets, complete=True, write_checksums=True)
            manifest = (directory / "SHA256SUMS").read_bytes()
            self.assertNotIn(b"\r", manifest)
            checksums = VERIFY["parse_checksums"](manifest, {RELEASE["release_name"]("1.2.3", target) for target in targets})
            for name, checksum in checksums.items():
                self.assertEqual(hashlib.sha256((directory / name).read_bytes()).hexdigest(), checksum)
            self.assertEqual(len(list(directory.iterdir())), 6)
            VERIFY["verify_release"](directory, "1.2.3", targets, complete=True)

    def test_checksum_verification_rejects_tampering_without_rewriting_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            target = "x86_64-unknown-linux-musl"
            file = directory / RELEASE["release_name"]("1.2.3", target)
            file.write_bytes(elf())
            VERIFY["verify_release"](directory, "1.2.3", [target], write_checksums=True)
            manifest = (directory / "SHA256SUMS").read_bytes()
            file.write_bytes(elf() + b"tampered")
            with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
                VERIFY["verify_release"](directory, "1.2.3", [target])
            self.assertEqual(manifest, (directory / "SHA256SUMS").read_bytes())

    def test_complete_release_rejects_extra_attachments_and_missing_platforms(self):
        for extra in ("old.zip", "source.tar.gz", "README.md", "mihomo-web-helper"):
            with tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                targets = release_files(directory)
                (directory / extra).write_bytes(b"unexpected")
                with self.assertRaisesRegex(RuntimeError, "exactly the five"):
                    VERIFY["verify_release"](directory, "1.2.3", targets, complete=True, write_checksums=True)
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            targets = release_files(directory)
            (directory / RELEASE["release_name"]("1.2.3", targets[-1])).unlink()
            with self.assertRaises(RuntimeError):
                VERIFY["verify_release"](directory, "1.2.3", targets, complete=True, write_checksums=True)
            with self.assertRaisesRegex(RuntimeError, "all five native targets"):
                VERIFY["verify_release"](directory, "1.2.3", targets[:-1], complete=True, write_checksums=True)

    def test_checksum_names_cannot_escape_the_release_or_duplicate_entries(self):
        checksum = "0" * 64
        for content in (f"{checksum}  ../private\n", f"{checksum}  app\n{checksum}  app\n", f"{checksum}  extra\n", "malformed\n"):
            with self.assertRaises(RuntimeError):
                VERIFY["parse_checksums"](content.encode(), {"app"})

    @unittest.skipIf(os.name == "nt", "Unprivileged Windows runners cannot create filesystem symlinks")
    def test_symlinks_are_rejected_at_staging_and_verification(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "compiled"
            source.write_bytes(elf())
            link = root / "linked"
            link.symlink_to(source)
            with self.assertRaises(RuntimeError):
                RELEASE["stage_binary"](link, root / "output", "x86_64-unknown-linux-musl")
            with self.assertRaises(RuntimeError):
                VERIFY["inspect_binary"](link, "x86_64-unknown-linux-musl")

    def test_source_policy_rejects_runtime_data(self):
        for name in (".tools/cache.txt", "target/app", "web/node_modules/package.json", "web/dist/index.html", "data/app.db", "config.toml", "nested/config.toml", "config.lock", "instance.lock", "bootstrap-token", "certificate.key"):
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


if __name__ == "__main__":
    unittest.main()
