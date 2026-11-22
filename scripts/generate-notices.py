"""Collect release dependency license texts from the installed, locked dependencies.

Requires Python 3.11+, Cargo and the frontend's installed node_modules. This only
reads dependency sources and writes notices inside this project's third_party.
"""
from __future__ import annotations

import argparse
import json
import os
import runpy
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DESTINATION = ROOT / "third_party" / "licenses"
POLICY = runpy.run_path(str(ROOT / "scripts/check-repository.py"))


def license_files(source: Path, explicit: str | None = None) -> list[Path]:
    result: set[Path] = set()
    for directory, children, names in os.walk(source):
        children[:] = [name for name in children if name not in ("node_modules", ".git", "target")]
        for name in names:
            if name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "PATENTS", "UNLICENSE")):
                file = Path(directory) / name
                if file.is_file() and not file.is_symlink():
                    result.add(file)
    if explicit:
        file = Path(explicit)
        if not file.is_absolute():
            file = source / file
        if file.is_file() and file.resolve().is_relative_to(source.resolve()):
            result.add(file)
    return sorted(result)


def collect(kind: str, name: str, version: str, source: Path, explicit: str | None = None) -> list[str]:
    links = []
    folder = DESTINATION / kind / f"{name.replace('/', '_')}-{version}"
    for original in license_files(source, explicit):
        relative = original.relative_to(source)
        destination = folder / relative
        if not destination.resolve().is_relative_to(DESTINATION.resolve()):
            raise RuntimeError("License destination is outside the project")
        destination.parent.mkdir(parents=True, exist_ok=True)
        content = original.read_bytes()
        POLICY["check_private_content"](destination.relative_to(ROOT).as_posix(), content)
        destination.write_bytes(content)
        links.append(destination.relative_to(ROOT).as_posix())
    if not links and folder.is_dir():
        # A few workspace packages omit their root license when publishing.
        # Preserve reviewed upstream texts and provenance shipped with this repo.
        links = [file.relative_to(ROOT).as_posix() for file in license_files(folder)]
    return links


def cell(value: str | None) -> str:
    return (value or "见依赖源文件").replace("|", "\\|").replace("\n", " ")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", help="Target triple; defaults to the current Rust host")
    arguments = parser.parse_args()
    if arguments.target is None:
        host = subprocess.check_output(["rustc", "-vV"], cwd=ROOT, text=True, encoding="utf-8")
        arguments.target = next(line.removeprefix("host: ") for line in host.splitlines() if line.startswith("host: "))
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", arguments.target],
        cwd=ROOT, text=True, encoding="utf-8",
    ))
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    project = metadata["resolve"]["root"]
    reachable: set[str] = set()
    pending = [project]
    while pending:
        package = pending.pop()
        if package in reachable:
            continue
        reachable.add(package)
        for dependency in nodes[package]["deps"]:
            if any(kind["kind"] in (None, "build") for kind in dependency["dep_kinds"]):
                pending.append(dependency["pkg"])
    reachable.remove(project)
    lines = [
        "# 第三方依赖与许可证", "",
        "本项目自身采用 MIT 许可证。以下清单依据 Cargo.lock、web/package-lock.json 和已安装的依赖生成。",
        f"Rust 清单包含 `{arguments.target}` 发布目标的生产与编译依赖；前端清单包含生产依赖及 Tailwind 构建组件。",
        "清单采用保守范围，部分编译工具不会进入最终程序。测试框架、临时虚拟机和下载的 API 参考材料不随生产程序分发。",
        "各依赖保留自己的许可证和版权声明，完整文本位于 third_party/licenses。更新锁文件后请重新生成。", "",
        "## Rust", "", "| 依赖 | 版本 | 许可证 | 随附文本 |", "| --- | --- | --- | --- |",
    ]
    missing: list[str] = []
    for identifier in sorted(reachable, key=lambda key: (packages[key]["name"], packages[key]["version"])):
        package = packages[identifier]
        texts = collect("rust", package["name"], package["version"], Path(package["manifest_path"]).parent, package.get("license_file"))
        if not texts:
            missing.append(f"rust:{package['name']}@{package['version']}")
        links = ", ".join(f"[{Path(link).name}]({link})" for link in texts) or "依赖仅提供 SPDX 元数据，见 crates.io 的原始包"
        lines.append(f"| [{package['name']}](https://crates.io/crates/{package['name']}/{package['version']}) | {package['version']} | {cell(package.get('license'))} | {links} |")
    lines.extend(["", "## 前端", "", "| 依赖 | 版本 | 许可证 | 随附文本 |", "| --- | --- | --- | --- |"])
    lock = json.loads((ROOT / "web/package-lock.json").read_text(encoding="utf-8"))
    frontend_count = 0
    for key, record in sorted(lock["packages"].items()):
        if not key.startswith("node_modules/"):
            continue
        if record.get("dev", False) and not key.startswith(("node_modules/tailwindcss", "node_modules/@tailwindcss/")):
            continue
        source = ROOT / "web" / key
        manifest = source / "package.json"
        if not manifest.exists():
            continue  # Optional packages for another OS are not distributed.
        package = json.loads(manifest.read_text(encoding="utf-8"))
        name, version = package["name"], package["version"]
        texts = collect("npm", name, version, source)
        if not texts:
            missing.append(f"npm:{name}@{version}")
        links = ", ".join(f"[{Path(link).name}]({link})" for link in texts) or "见 npm 原始包中的声明"
        license_name = package.get("license", record.get("license"))
        if isinstance(license_name, dict):
            license_name = license_name.get("type")
        lines.append(f"| [{name}](https://www.npmjs.com/package/{name}/v/{version}) | {version} | {cell(license_name)} | {links} |")
        frontend_count += 1
    lines.extend([
        "", "## 原生组件", "",
        "`libsqlite3-sys` 链接的 SQLite 源代码属于公有领域，参见 [SQLite 官方版权说明](https://www.sqlite.org/copyright.html)。",
        "Rust TLS、压缩等库内嵌的第三方声明一并从依赖原包收集；此目录不包含 Mihomo 内核或其源码。", "",
    ])
    (ROOT / "THIRD_PARTY_NOTICES.md").write_bytes("\n".join(lines).encode("utf-8"))
    print(f"Collected licenses: {len(reachable)} Rust dependencies, {frontend_count} frontend dependencies")
    if missing:
        raise RuntimeError("Missing dependency license text: " + ", ".join(missing))


if __name__ == "__main__":
    main()
