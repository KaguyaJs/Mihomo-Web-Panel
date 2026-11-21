"""Validate a release tag and produce metadata and notes for GitHub Actions."""
from __future__ import annotations

import argparse
import json
import os
import runpy
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--notes-output", default=".tools/release-notes.md")
    arguments = parser.parse_args()
    package = runpy.run_path(str(ROOT / "scripts/package-release.py"))
    version = package["version_from_project"]()
    package["validate_tag"](arguments.tag, version)
    runpy.run_path(str(ROOT / "scripts/check-repository.py"))["versions"]()
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    tagged = subprocess.check_output(["git", "rev-parse", "--verify", f"refs/tags/{arguments.tag}^{{commit}}"], cwd=ROOT, text=True).strip()
    if head != tagged:
        raise RuntimeError("Checkout must point to the requested release tag")
    timestamp = subprocess.check_output(["git", "log", "-1", "--format=%ct"], cwd=ROOT, text=True).strip()
    changelog = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    section = changelog.split(f"## [{version}]", 1)[1].split("\n## ", 1)[0].split("\n[", 1)[0].strip()
    notes = f"# mihomo-web-panel {arguments.tag}\n\n{section}\n\n提供 Linux x86_64 / ARM64、Windows x64、macOS Intel / Apple Silicon 的独立可执行程序，网页和 SQLite 均内嵌。解压后可直接启动，无需 Rust 或 Node.js。\n\n发布文件经过目标架构测试、独立启动与持久化验证和 SHA256 校验；Linux 另执行真实 systemd 部署检查。直接运行见 [使用说明](https://github.com/KaguyaJs/mihomo-web-panel/blob/{arguments.tag}/docs/standalone.md)。\n"
    destination = ROOT / arguments.notes_output
    if not destination.resolve().is_relative_to(ROOT.resolve()):
        raise RuntimeError("Release notes must stay inside the checkout")
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(notes, encoding="utf-8")
    output = {"tag": arguments.tag, "version": version, "commit": head, "timestamp": timestamp, "prerelease": str("-" in version).lower()}
    output["matrix"] = json.dumps({"include": [
        {"target": target, "platform": info["platform"], "arch": info["architecture"], "runner": info["runner"], "extension": info["format"]}
        for target, info in package["TARGETS"].items()
    ]}, separators=(",", ":"))
    if "GITHUB_OUTPUT" in os.environ:
        with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as file:
            for key, value in output.items():
                file.write(f"{key}={value}\n")
    print(f"Validated {arguments.tag} at {head}")


if __name__ == "__main__":
    main()
