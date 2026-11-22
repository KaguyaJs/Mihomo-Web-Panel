"""Stage one directly downloadable executable for a native release target."""
from __future__ import annotations

import argparse
import re
import runpy
import struct
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
POLICY = runpy.run_path(str(ROOT / "scripts/check-repository.py"))
TARGETS = {
    "x86_64-unknown-linux-musl": {"platform": "linux", "architecture": "x86_64", "machine": 62, "runner": "ubuntu-24.04"},
    "aarch64-unknown-linux-musl": {"platform": "linux", "architecture": "aarch64", "machine": 183, "runner": "ubuntu-24.04-arm"},
    "x86_64-pc-windows-msvc": {"platform": "windows", "architecture": "x86_64", "machine": 0x8664, "runner": "windows-latest"},
    "x86_64-apple-darwin": {"platform": "macos", "architecture": "x86_64", "machine": 0x01000007, "runner": "macos-15-intel"},
    "aarch64-apple-darwin": {"platform": "macos", "architecture": "aarch64", "machine": 0x0100000C, "runner": "macos-15"},
}


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
    return release_prefix(version, target) + (".exe" if TARGETS[target]["platform"] == "windows" else "")


def validate_binary(data: bytes, target: str) -> None:
    info = TARGETS[target]
    validator = {"linux": validate_elf, "windows": validate_pe, "macos": validate_macho}[info["platform"]]
    validator(data, info["machine"])


def version_from_project() -> str:
    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    if not re.fullmatch(r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?", version):
        raise RuntimeError("Package version is not a supported semantic version")
    return version


def validate_tag(tag: str, version: str) -> None:
    if tag != "v" + version:
        raise RuntimeError("Release tag must match the package version exactly")


def stage_binary(source: Path, destination: Path, target: str) -> None:
    if source.is_symlink() or not source.is_file() or not 0 < source.stat().st_size <= 64 * 1024 * 1024:
        raise RuntimeError("Release input must be a regular executable of a reasonable size")
    content = source.read_bytes()
    validate_binary(content, target)
    POLICY["check_private_content"](destination.name, content)
    if destination.is_symlink():
        raise RuntimeError("Release destination must not be a symlink")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=destination.parent, prefix=".release-", suffix=".tmp", delete=False) as output:
        temporary = Path(output.name)
        output.write(content)
    try:
        temporary.chmod(0o755)
        temporary.replace(destination)
    finally:
        temporary.unlink(missing_ok=True)
    print(f"Staged executable {destination.name}: {len(content):,} bytes")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, choices=TARGETS)
    parser.add_argument("--directory", type=Path, default=ROOT / "dist")
    parser.add_argument("--tag", help="Require an exact vX.Y.Z version match")
    arguments = parser.parse_args()
    version = version_from_project()
    if arguments.tag:
        validate_tag(arguments.tag, version)
    source = ROOT / "target" / arguments.target / "release" / binary_name(arguments.target)
    destination = arguments.directory / release_name(version, arguments.target)
    stage_binary(source, destination, arguments.target)


if __name__ == "__main__":
    main()
