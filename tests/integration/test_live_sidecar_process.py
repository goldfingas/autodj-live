"""Real-process tests for the private AutoDJ Live sidecar command."""

from __future__ import annotations

import http.client
import json
import os
import signal
import socket
import subprocess
import sys
import time
from collections.abc import Iterator
from contextlib import contextmanager
from typing import Any
from uuid import UUID

TOKEN = "ab" * 32
SESSION_ID = "11111111-2222-3333-4444-555555555555"
TIMEOUT = 10.0


def _environment(*, token: str | None = TOKEN) -> dict[str, str]:
    environment = os.environ.copy()
    environment["AUTODJ_RUNTIME"] = "current"
    environment["AUTODJ_LIVE_SESSION_ID"] = SESSION_ID
    if token is None:
        environment.pop("AUTODJ_LIVE_TOKEN", None)
    else:
        environment["AUTODJ_LIVE_TOKEN"] = token
    return environment


def _command() -> list[str]:
    return [
        sys.executable,
        "-m",
        "autodj",
        "_live-sidecar",
        "--host",
        "127.0.0.1",
        "--port",
        "0",
    ]


def _start() -> subprocess.Popen[str]:
    creationflags = subprocess.CREATE_NEW_PROCESS_GROUP if os.name == "nt" else 0
    return subprocess.Popen(
        _command(),
        env=_environment(),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        creationflags=creationflags,
    )


@contextmanager
def _sidecar() -> Iterator[tuple[subprocess.Popen[str], dict[str, Any]]]:
    process = _start()
    try:
        assert process.stdout is not None
        first_line = process.stdout.readline()
        assert first_line, process.stderr.read() if process.stderr is not None else ""
        descriptor = json.loads(first_line)
        yield process, descriptor
    finally:
        if process.poll() is None:
            try:
                _graceful_stop(process)
            except (OSError, subprocess.TimeoutExpired):
                process.kill()
        process.wait(timeout=TIMEOUT)


def _health(port: int) -> tuple[int, dict[str, Any], dict[str, str]]:
    deadline = time.monotonic() + TIMEOUT
    while True:
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=1.0)
        try:
            connection.request(
                "GET",
                "/live/v1/health",
                headers={
                    "Authorization": f"Bearer {TOKEN}",
                    "X-AutoDJ-Live-Session": SESSION_ID,
                },
            )
            response = connection.getresponse()
            body = json.loads(response.read())
            headers = {name.casefold(): value for name, value in response.getheaders()}
            return response.status, body, headers
        except (ConnectionError, OSError, http.client.HTTPException):
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.05)
        finally:
            connection.close()


def _graceful_stop(process: subprocess.Popen[str]) -> tuple[str, str]:
    if os.name == "nt":
        process.send_signal(signal.CTRL_BREAK_EVENT)
    else:
        process.send_signal(signal.SIGINT)
    return process.communicate(timeout=TIMEOUT)


def test_private_command_is_hidden_from_normal_help() -> None:
    completed = subprocess.run(
        [sys.executable, "-m", "autodj", "--help"],
        env=_environment(),
        check=True,
        capture_output=True,
        text=True,
        timeout=TIMEOUT,
    )

    assert "_live-sidecar" not in completed.stdout


def test_sidecar_uses_one_redacted_descriptor_and_an_ephemeral_loopback_listener() -> None:
    with _sidecar() as (process, descriptor):
        assert descriptor == {
            "protocol": {"major": 1, "minor": 0},
            "port": descriptor["port"],
            "pid": descriptor["pid"],
            "sidecar_id": descriptor["sidecar_id"],
        }
        assert isinstance(descriptor["port"], int) and descriptor["port"] > 0
        assert isinstance(descriptor["pid"], int) and descriptor["pid"] > 0
        UUID(descriptor["sidecar_id"])

        with socket.socket(socket.AF_INET6, socket.SOCK_STREAM) as ipv6:
            ipv6.settimeout(0.25)
            assert ipv6.connect_ex(("::1", descriptor["port"])) != 0

        status, body, headers = _health(descriptor["port"])
        assert status == 200
        assert body == {
            "protocol": {"major": 1, "minor": 0},
            "session_id": SESSION_ID,
            "sidecar_id": descriptor["sidecar_id"],
            "status": "healthy",
        }
        assert headers["cache-control"] == "no-store"

        stdout, stderr = _graceful_stop(process)
        expected_codes = {0, 3} if os.name == "nt" else {0}
        assert process.returncode in expected_codes, stderr
        assert stdout == ""
        assert "Application shutdown complete." in stderr
        assert "Traceback" not in stderr
        assert TOKEN not in json.dumps(descriptor)
        assert TOKEN not in stderr


def test_missing_launch_token_fails_before_emitting_a_descriptor() -> None:
    completed = subprocess.run(
        _command(),
        env=_environment(token=None),
        check=False,
        capture_output=True,
        text=True,
        timeout=TIMEOUT,
    )

    assert completed.returncode != 0
    assert completed.stdout == ""
    assert "AUTODJ_LIVE_TOKEN" in completed.stderr
    assert "64 lowercase hex characters" in completed.stderr
