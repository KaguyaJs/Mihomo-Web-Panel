"""Validate release source links, versions, and the absence of private runtime data."""
from __future__ import annotations

import json
import re
import runpy
import subprocess
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PRIVATE_CONTENT = [
    re.compile(r"(?<![A-Za-z0-9])[A-Z]:[\\/]"),
    re.compile(r"/(?:Users|home)/[^\s/]+/"),
    re.compile(r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{30,})\b"),
    re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----"),
]
FORBIDDEN_PARTS = {".tools", "target", "node_modules", "playwright-report", "test-results", "__pycache__"}


def check_private_content(name: str, data: bytes) -> None:
    try:
        text = data.decode("utf-8-sig")
    except UnicodeDecodeError:
        # Executable and image metadata can also contain private paths. Scan
        # printable runs instead of treating arbitrary machine code as text.
        text = "\n".join(run.decode("ascii") for run in re.findall(rb"[\x20-\x7e]{8,}", data))
    if any(pattern.search(text) for pattern in PRIVATE_CONTENT):
        raise RuntimeError(f"Private path or credential pattern in {name}")


def check_source_name(name: str) -> None:
    file = Path(name)
    if FORBIDDEN_PARTS.intersection(file.parts) or name.startswith(("dist/", "data/")):
        raise RuntimeError(f"Runtime or generated file is tracked: {name}")
    if (file.name == "config.toml" and file.as_posix() != ".cargo/config.toml") or file.name in ("config.lock", "instance.lock", "bootstrap-token") or file.name.startswith(".env") or file.suffix in (".db", ".log", ".key", ".pem", ".p12", ".pfx", ".pyc") or file.name.endswith((".db-wal", ".db-shm", ".tsbuildinfo")):
        raise RuntimeError(f"Private runtime file is tracked: {name}")


def versions() -> str:
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    version = cargo["package"]["version"]
    frontend = json.loads((ROOT / "web/package.json").read_text(encoding="utf-8"))
    lock = json.loads((ROOT / "web/package-lock.json").read_text(encoding="utf-8"))
    if any(value != version for value in (frontend["version"], lock["version"], lock["packages"][""]["version"])):
        raise RuntimeError("Cargo and frontend package versions must match")
    cargo_lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    project = next(package for package in cargo_lock["package"] if package["name"] == cargo["package"]["name"])
    if project["version"] != version:
        raise RuntimeError("Cargo.lock does not match Cargo.toml")
    if f"## [{version}]" not in (ROOT / "CHANGELOG.md").read_text(encoding="utf-8"):
        raise RuntimeError("CHANGELOG.md is missing the current version")
    return version


def main() -> None:
    version = versions()
    if (ROOT / ".git").exists():
        names = [name for name in subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).decode().split("\0") if name]
    else:
        package = runpy.run_path(str(ROOT / "scripts/package-release.py"))
        names = list(package["source_inputs"]())
    if not names:
        raise RuntimeError("No source files to validate")
    for name in names:
        check_source_name(name)
        file = ROOT / name
        if file.is_symlink() or not file.is_file():
            raise RuntimeError(f"Source file is missing or is a symlink: {name}")
        check_private_content(name, file.read_bytes())
    for name in names:
        if not name.endswith(".md") or name.startswith("third_party/"):
            continue
        file = ROOT / name
        for link in re.findall(r"!?\[[^\]]*\]\(([^)]+)\)", file.read_text(encoding="utf-8")):
            link = link.strip("<>").split("#", 1)[0]
            if not link or re.match(r"^[a-z]+:", link):
                continue
            if not (file.parent / link).is_file():
                raise RuntimeError(f"Broken local Markdown link in {name}: {link}")
    print(f"Validated {len(names)} source files, documentation links and version {version}")


if __name__ == "__main__":
    main()
