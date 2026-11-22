"""Build and test native release programs without leaking build-machine paths."""
from __future__ import annotations

import argparse
import os
import runpy
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGETS = runpy.run_path(str(ROOT / "scripts/prepare-release.py"))["TARGETS"]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, choices=TARGETS)
    parser.add_argument("--skip-tests", action="store_true")
    arguments = parser.parse_args()
    environment = os.environ.copy()
    flags = environment.get("CARGO_ENCODED_RUSTFLAGS", "").split("\x1f") if environment.get("CARGO_ENCODED_RUSTFLAGS") else []
    flags.append(f"--remap-path-prefix={ROOT}=.")
    for variable, default in (("CARGO_HOME", Path.home() / ".cargo"), ("RUSTUP_HOME", Path.home() / ".rustup")):
        path = Path(environment.get(variable, default))
        flags.append(f"--remap-path-prefix={path}={variable.lower()}")
    platform = TARGETS[arguments.target]["platform"]
    if platform == "windows":
        flags.extend(["-C", "target-feature=+crt-static"])
    if platform == "macos":
        environment.setdefault("MACOSX_DEPLOYMENT_TARGET", "13.0")
    if platform == "linux":
        key = arguments.target.replace("-", "_")
        environment[f"CC_{key}"] = "musl-gcc"
        environment[f"CARGO_TARGET_{key.upper()}_LINKER"] = "musl-gcc"
    environment["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    subprocess.run([sys.executable, str(ROOT / "scripts/generate-notices.py"), "--target", arguments.target],
                   cwd=ROOT, env=environment, check=True)
    common = ["--release", "--locked", "--target", arguments.target]
    subprocess.run(["cargo", "build", *common, "--bins", "--examples"], cwd=ROOT, env=environment, check=True)
    if not arguments.skip_tests:
        subprocess.run(["cargo", "test", *common, "--all-targets"], cwd=ROOT, env=environment, check=True)


if __name__ == "__main__":
    main()
