"""Actual Linux/systemd deployment smoke test for an isolated VM or CI runner.

Run as root only on a disposable test machine. It installs/uninstalls this project,
starts the test-only Mock, and validates the real Web -> socket -> systemd path.
"""
from __future__ import annotations

import json
import os
import pwd
import socket
import struct
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SECRET = "mock-controller-secret-for-tests"
ORIGIN = "https://panel.example.com"
PASSWORD = "systemd integration test password"
SOCKET = "/run/mihomo-web-helper/control.sock"


def command(*arguments: str) -> str:
    return subprocess.check_output(arguments, text=True).strip()


def request(path: str, method: str = "GET", body: dict | None = None, cookie: str = "", csrf: str = "") -> tuple[dict, str]:
    headers = {"Host": "panel.example.com", "Origin": ORIGIN, "Content-Type": "application/json"}
    if cookie:
        headers["Cookie"] = cookie
    if csrf:
        headers["X-CSRF-Token"] = csrf
    payload = None if body is None else json.dumps(body).encode()
    entry = urllib.request.Request("http://127.0.0.1:8787" + path, data=payload, headers=headers, method=method)
    with urllib.request.urlopen(entry, timeout=15) as response:
        result = response.read().decode()
        assert SECRET not in result and "127.0.0.1:19090" not in result
        return (json.loads(result) if result else {}), response.headers.get("Set-Cookie", "")


def rejected_peer(uid: int, gid: int, payload: bytes) -> None:
    process = os.fork()
    if process == 0:
        try:
            os.setgroups([gid])
            os.setgid(gid)
            os.setuid(uid)
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
                stream.settimeout(5)
                stream.connect(SOCKET)
                stream.sendall(payload)
                try:
                    result = stream.recv(2048)
                except ConnectionResetError:
                    result = b""
                assert result == b"", "helper accepted an unauthorized request"
            os._exit(0)
        except BaseException:
            os._exit(1)
    _, result = os.waitpid(process, 0)
    assert os.waitstatus_to_exitcode(result) == 0


def main() -> None:
    assert os.geteuid() == 0 and Path("/run/systemd/system").exists()
    binaries = Path(os.environ.get("MIHOMO_SMOKE_BIN_DIR", str(ROOT / "target/release")))
    mock_binary = Path(os.environ.get("MIHOMO_SMOKE_MOCK", str(ROOT / "target/release/examples/mock_mihomo")))
    mock = subprocess.Popen([str(mock_binary)], env={**os.environ, "MOCK_BIND": "127.0.0.1:19090"}, stdout=subprocess.DEVNULL)
    try:
        subprocess.run(["bash", "-n", str(ROOT / "scripts/install.sh"), str(ROOT / "scripts/uninstall.sh")], check=True)
        subprocess.run(["bash", str(ROOT / "scripts/install.sh"), "--origin", ORIGIN, "--binary-dir", str(binaries)], check=True)
        for _ in range(15):
            try:
                assert request("/api/health")[0]["status"] == "ok"
                break
            except (OSError, urllib.error.URLError):
                time.sleep(1)
        else:
            raise RuntimeError("web service did not become ready")
        account = pwd.getpwnam("mihomo-web")
        assert account.pw_uid != 0
        pid = int(command("systemctl", "show", "mihomo-web.service", "-p", "MainPID", "--value"))
        status = Path(f"/proc/{pid}/status").read_text()
        assert f"Uid:\t{account.pw_uid}\t" in status
        assert "NoNewPrivs:\t1" in status
        for file in ("app.db", "bootstrap-token"):
            metadata = Path("/var/lib/mihomo-web", file).stat()
            assert metadata.st_mode & 0o777 == 0o600
            assert metadata.st_uid == account.pw_uid
        config_path = Path("/etc/mihomo-web/config.toml")
        assert config_path.stat().st_mode & 0o777 == 0o640
        assert config_path.stat().st_uid == 0 and config_path.stat().st_gid == account.pw_gid
        subprocess.run(["systemd-analyze", "verify", "/etc/systemd/system/mihomo-web.service", "/etc/systemd/system/mihomo-web-helper.service", "/etc/systemd/system/mihomo-web-helper.socket"], check=True)
        token = Path("/var/lib/mihomo-web/bootstrap-token").read_text().strip()
        request("/api/setup/complete", "POST", {"token": token, "username": "ci-admin", "password": PASSWORD, "confirm_password": PASSWORD, "controller_url": "http://127.0.0.1:19090", "secret": SECRET})
        user, set_cookie = request("/api/auth/login", "POST", {"username": "ci-admin", "password": PASSWORD})
        assert "HttpOnly" in set_cookie and "Secure" in set_cookie and "SameSite=Strict" in set_cookie
        cookie = set_cookie.split(";", 1)[0]
        csrf = user["csrf_token"]
        original = request("/api/settings/autostart", cookie=cookie)[0]
        assert original["supported"] and original["active"] == "active"
        for operation, expected in [("disable", False), ("enable", True)]:
            result = request("/api/settings/autostart", "POST", {"operation": operation}, cookie, csrf)[0]
            assert result["supported"] and result["enabled"] is expected and result["active"] == "active"
            actual = subprocess.run(["systemctl", "is-enabled", "mihomo-web.service"], stdout=subprocess.PIPE, text=True)
            assert (actual.stdout.strip() == "enabled") is expected
            assert command("systemctl", "is-active", "mihomo-web.service") == "active"
        # Reinstallation must preserve the administrator, backend and session,
        # while replacing both running executables with the new installation.
        previous_config = config_path.read_bytes()
        helper_pid = command("systemctl", "show", "mihomo-web-helper.service", "-p", "MainPID", "--value")
        subprocess.run(["bash", str(ROOT / "scripts/install.sh"), "--binary-dir", str(binaries)], check=True)
        assert config_path.read_bytes() == previous_config
        assert Path("/usr/local/bin/mihomo-web-panel.previous").exists()
        assert Path("/usr/local/libexec/mihomo-web-helper.previous").exists()
        assert command("systemctl", "show", "mihomo-web.service", "-p", "MainPID", "--value") != str(pid)
        assert command("systemctl", "show", "mihomo-web-helper.service", "-p", "MainPID", "--value") != helper_pid
        for _ in range(15):
            try:
                assert request("/api/auth/me", cookie=cookie)[0]["username"] == "ci-admin"
                assert request("/api/status", cookie=cookie)[0]["connected"]
                break
            except (OSError, urllib.error.URLError):
                time.sleep(1)
        else:
            raise RuntimeError("upgraded service did not preserve working authentication and backend")
        # File group access cannot bypass the exact caller UID check; root is rejected too.
        rejected_peer(0, 0, b'{"operation":"status"}\n')
        rejected_peer(54321, account.pw_gid, b'{"operation":"status"}\n')
        rejected_peer(account.pw_uid, account.pw_gid, b'{"operation":"enable","unit":"ssh.service"}\n')
        rejected_peer(account.pw_uid, account.pw_gid, b'{"operation":"restart"}\n')
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
            stream.connect(SOCKET)
            _, peer_uid, _ = struct.unpack("3i", stream.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
            assert peer_uid == 0
        logs = command("journalctl", "-u", "mihomo-web.service", "-u", "mihomo-web-helper.service", "--no-pager")
        assert SECRET not in logs and PASSWORD not in logs
        subprocess.run(["bash", str(ROOT / "scripts/uninstall.sh")], check=True)
        assert Path("/var/lib/mihomo-web/app.db").exists()
        assert Path("/etc/mihomo-web/config.toml").exists()
        assert not Path("/usr/local/bin/mihomo-web-panel").exists()
        print("SYSTEMD_VALIDATION_PASSED: real API, root helper, UID checks, unit hardening, enable/disable, preserved upgrade and uninstall")
    except BaseException:
        diagnostics = subprocess.run(["journalctl", "-u", "mihomo-web.service", "-u", "mihomo-web-helper.service", "-n", "100", "--no-pager"], stdout=subprocess.PIPE, text=True, check=False)
        print(diagnostics.stdout.replace(SECRET, "[redacted]").replace(PASSWORD, "[redacted]"), flush=True)
        raise
    finally:
        mock.terminate()
        mock.wait(timeout=10)


if __name__ == "__main__":
    main()
