"""Verify an actual standalone executable, its archive and persisted first-run state.

Runs as a regular user on Linux, Windows or macOS. Only a loopback Mock is started;
this test never installs or starts Mihomo or a system service.
"""
from __future__ import annotations

import argparse
import json
import os
import runpy
import shutil
import signal
import socket
import sqlite3
import stat
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from contextlib import closing
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PACKAGE = runpy.run_path(str(ROOT / "scripts/package-release.py"))
VERIFY = runpy.run_path(str(ROOT / "scripts/verify-archives.py"))
SECRET = "mock-controller-secret-for-tests"
PASSWORD = "standalone integration test password"


def free_port() -> int:
    with socket.socket() as stream:
        stream.bind(("127.0.0.1", 0))
        return stream.getsockname()[1]


def request(origin: str, path: str, method: str = "GET", body=None, cookie: str = "", csrf: str = ""):
    headers = {"Origin": origin, "Content-Type": "application/json"}
    if cookie:
        headers["Cookie"] = cookie
    if csrf:
        headers["X-CSRF-Token"] = csrf
    entry = urllib.request.Request(origin + path, None if body is None else json.dumps(body).encode(), headers, method=method)
    with urllib.request.urlopen(entry, timeout=30) as response:
        data = response.read().decode()
        assert SECRET not in data, "Controller credentials appeared in an API response"
        return (json.loads(data) if data else {}), response.headers


def ready(process: subprocess.Popen, origin: str) -> None:
    deadline = time.monotonic() + 35
    while time.monotonic() < deadline:
        assert process.poll() is None, "Standalone process exited before becoming ready"
        try:
            assert request(origin, "/api/health")[0]["status"] == "ok"
            return
        except (OSError, urllib.error.URLError):
            time.sleep(0.15)
    raise RuntimeError("Standalone service did not become ready")


def stop(process: subprocess.Popen) -> None:
    if process.poll() is not None:
        return
    if os.name == "nt":
        process.send_signal(signal.CTRL_BREAK_EVENT)
    else:
        process.send_signal(signal.SIGINT)
    try:
        process.wait(timeout=15)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=10)
        raise RuntimeError("Standalone process did not shut down gracefully")
    assert process.returncode == 0, "Standalone shutdown failed"


def launch(binary: Path, arguments: list[str], cwd: Path, environment: dict, log):
    return subprocess.Popen([str(binary), *arguments], cwd=cwd, env=environment, stdout=log, stderr=subprocess.STDOUT,
                            creationflags=subprocess.CREATE_NEW_PROCESS_GROUP if os.name == "nt" else 0)


def check_private(directory: Path) -> None:
    if os.name != "nt":
        assert stat.S_IMODE(directory.stat().st_mode) == 0o700
        for name in ("app.db", "bootstrap-token"):
            path = directory / name
            if path.exists():
                assert stat.S_IMODE(path.stat().st_mode) == 0o600
    else:
        # Use .NET's ACL reader on the real paths, without printing user identities.
        # Every allowed SID is SYSTEM, Administrators or the current process user;
        # Users/Everyone and inherited shared-folder grants must be absent.
        script = """$ErrorActionPreference='Stop'
$identity=[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$allowed=@($identity,'S-1-5-18','S-1-5-32-544')
$paths=@($env:MIHOMO_SMOKE_DATA,(Join-Path $env:MIHOMO_SMOKE_DATA 'app.db'))
foreach($path in $paths){
  $acl=Get-Acl -LiteralPath $path
  if(-not $acl.AreAccessRulesProtected){throw 'Data ACL inherits parent grants'}
  foreach($rule in $acl.Access){
    $sid=$rule.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value
    if($rule.AccessControlType -eq 'Allow' -and $sid -notin $allowed){throw 'Unexpected private-data grant'}
  }
}
"""
        environment = {**os.environ, "MIHOMO_SMOKE_DATA": str(directory)}
        # PowerShell 7's inherited module path is incompatible with Windows
        # PowerShell 5.1. Let the selected executable initialize its own modules.
        environment.pop("PSModulePath", None)
        result = subprocess.run([shutil.which("pwsh") or "powershell.exe", "-NoProfile", "-NonInteractive", "-Command", script],
                                env=environment, capture_output=True)
        if result.returncode:
            diagnostic = result.stderr.decode(errors="replace").replace(str(directory), "[private data]")
            raise RuntimeError("Windows private-file ACL verification failed: " + diagnostic)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=PACKAGE["TARGETS"])
    parser.add_argument("--binary-dir", default="target/debug")
    arguments = parser.parse_args()
    platform = PACKAGE["TARGETS"][arguments.target]["platform"] if arguments.target else ("windows" if os.name == "nt" else "macos" if __import__("sys").platform == "darwin" else "linux")
    suffix = ".exe" if platform == "windows" else ""
    version = PACKAGE["version_from_project"]()
    input_directory = ROOT / "target" / arguments.target / "release" if arguments.target else ROOT / arguments.binary_dir
    binary_input = input_directory / ("mihomo-web-panel" + suffix)
    mock_input = input_directory / "examples" / ("mock_mihomo" + suffix)
    with tempfile.TemporaryDirectory(prefix="mihomo-standalone-") as temporary:
        root = Path(temporary)
        program = root / "portable program 空间"
        unrelated = root / "other working directory"
        program.mkdir()
        unrelated.mkdir()
        if arguments.target:
            prefix = PACKAGE["release_prefix"](version, arguments.target)
            archive = ROOT / "dist" / PACKAGE["release_name"](version, arguments.target)
            VERIFY["inspect_archive"](archive, prefix, arguments.target)
            contents = VERIFY["read_archive"](archive, prefix)
            for name, content in contents.items():
                destination = program / name
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(content)
                if os.name != "nt":
                    destination.chmod(PACKAGE["file_mode"](name))
        else:
            shutil.copy2(binary_input, program / binary_input.name)
        binary = program / binary_input.name
        assert version in subprocess.check_output([str(binary), "--version"], text=True)
        environment = {**os.environ, "HOME": str(root / "user"), "LOCALAPPDATA": str(root / "local-data"),
                       "XDG_CONFIG_HOME": str(root / "xdg-config"), "XDG_DATA_HOME": str(root / "xdg-data")}
        def cli(*parts):
            return subprocess.run([str(binary), *parts], cwd=unrelated, env=environment, text=True, encoding="utf-8", capture_output=True)
        paths = cli("paths")
        assert paths.returncode == 0
        managed = json.loads(paths.stdout)
        config = Path(managed["config"])
        data = Path(managed["data"])
        assert config.is_relative_to(root) and data.is_relative_to(root)
        assert not config.exists() and not data.exists(), "paths command modified the filesystem"
        log_path = root / "startup.log"
        children = []
        with log_path.open("wb") as log:
            try:
                # The default executable creates its own configuration and uses
                # the embedded UI even from an unrelated working directory.
                process = launch(binary, [], unrelated, environment, log)
                children.append(process)
                ready(process, "http://127.0.0.1:8787")
                assert config.exists() and data.joinpath("bootstrap-token").exists()
                with urllib.request.urlopen("http://127.0.0.1:8787", timeout=10) as response:
                    assert b"id=\"app\"" in response.read()
                check_private(data)
                token = cli("bootstrap-token")
                assert token.returncode == 0 and token.stdout.strip() == data.joinpath("bootstrap-token").read_text().strip()
                stop(process)
                saved = config.read_bytes()
                process = launch(binary, ["serve", "--no-open"], unrelated, environment, log)
                children.append(process)
                ready(process, "http://127.0.0.1:8787")
                assert config.read_bytes() == saved
                assert cli("bootstrap-token").stdout.strip() == token.stdout.strip()
                stop(process)
                # Portable state is bound to the executable directory and stays
                # there when launched from a different terminal directory.
                process = launch(binary, ["--portable", "--no-open"], unrelated, environment, log)
                children.append(process)
                ready(process, "http://127.0.0.1:8787")
                portable = json.loads(cli("--portable", "paths").stdout)
                assert Path(portable["config"]) == program / "config.toml"
                assert Path(portable["data"]) == program / "data"
                data = program / "data"
                token = cli("--portable", "bootstrap-token").stdout.strip()
                stop(process)
                port, mock_port = free_port(), free_port()
                while mock_port == port:
                    mock_port = free_port()
                origin = f"http://127.0.0.1:{port}"
                controller = f"http://127.0.0.1:{mock_port}"
                portable_config = program / "config.toml"
                portable_config.write_text(f'listen = "127.0.0.1:{port}"\npublic_origin = "{origin}"\ndata_dir = "data"\n', encoding="utf-8")
                mock = launch(mock_input, [], root, {**environment, "MOCK_BIND": f"127.0.0.1:{mock_port}"}, log)
                children.append(mock)
                process = launch(binary, ["--portable", "--no-open"], unrelated, environment, log)
                children.append(process)
                ready(process, origin)
                # A second process cannot reuse this instance's private data.
                duplicate = cli("--portable", "serve", "--no-open")
                assert duplicate.returncode != 0 and "another service instance" in duplicate.stderr
                tested = request(origin, "/api/setup/test", "POST", {"token": token, "controller_url": controller, "secret": SECRET})[0]
                assert tested["version"] == "v1.19.20"
                request(origin, "/api/setup/complete", "POST", {"token": token, "username": "smoke-admin", "password": PASSWORD, "confirm_password": PASSWORD, "controller_url": controller, "secret": SECRET})
                user, headers = request(origin, "/api/auth/login", "POST", {"username": "smoke-admin", "password": PASSWORD})
                cookie = headers["Set-Cookie"].split(";", 1)[0]
                csrf = user["csrf_token"]
                assert request(origin, "/api/status", cookie=cookie)[0]["connected"]
                request(origin, "/api/proxies/PROXY", "PUT", {"name": "日本 / Tokyo 02"}, cookie, csrf)
                autostart = request(origin, "/api/settings/autostart", cookie=cookie)[0]
                assert not autostart["supported"]
                request(origin, "/api/settings", "PUT", {"listen": f"127.0.0.1:{port}", "public_origin": origin, "session_idle_seconds": 1800, "session_absolute_seconds": 86400, "traffic_interval_ms": 2000}, cookie, csrf)
                assert not data.joinpath("bootstrap-token").exists()
                assert cli("--portable", "bootstrap-token").returncode != 0
                previous_config = portable_config.read_bytes()
                stop(process)
                process = launch(binary, ["--portable", "serve", "--no-open"], root, environment, log)
                children.append(process)
                ready(process, origin)
                assert request(origin, "/api/auth/me", cookie=cookie)[0]["username"] == "smoke-admin"
                assert request(origin, "/api/status", cookie=cookie)[0]["connected"]
                assert request(origin, "/api/settings", cookie=cookie)[0]["settings"]["traffic_interval_ms"] == 2000
                assert portable_config.read_bytes() == previous_config
                check_private(data)
                stop(process)
                # WAL shutdown leaves a valid database; no logged bootstrap or
                # controller secret is allowed when stdout is redirected.
                with closing(sqlite3.connect(data / "app.db")) as database:
                    assert database.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
                log.flush()
                assert token.encode() not in log_path.read_bytes()
                assert SECRET.encode() not in log_path.read_bytes()
                print(f"STANDALONE_VALIDATION_PASSED: {platform}, embedded UI, default/portable paths, private files, initialization, API, persistent session/settings and graceful restart")
            finally:
                for child in reversed(children):
                    if child.poll() is None:
                        child.kill()
                        child.wait(timeout=10)


if __name__ == "__main__":
    main()
