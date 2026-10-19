"""Exercise a staged sidecar with only the environment an installed app receives."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
import urllib.request
from pathlib import Path
from uuid import uuid4


def _clean_environment(token: str, session_id: str) -> dict[str, str]:
    """Keep the startup secret while removing development Python configuration."""
    environment = {
        "AUTODJ_LIVE_TOKEN": token,
        "AUTODJ_LIVE_SESSION_ID": session_id,
        "PATH": os.environ.get("SYSTEMROOT", r"C:\Windows"),
        "SYSTEMROOT": os.environ.get("SYSTEMROOT", r"C:\Windows"),
        "WINDIR": os.environ.get("WINDIR", r"C:\Windows"),
    }
    return environment


def _health(port: int, token: str, session_id: str) -> dict[str, object]:
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/live/v1/health",
        headers={
            "Authorization": f"Bearer {token}",
            "X-AutoDJ-Live-Session": session_id,
        },
    )
    with urllib.request.urlopen(request, timeout=1) as response:
        return json.loads(response.read())


def main(runtime: Path) -> None:
    python = runtime / "python.exe"
    token = "2f" * 32
    session_id = str(uuid4())
    process = subprocess.Popen(
        [str(python), "-I", "-m", "autodj", "_live-sidecar", "--host", "127.0.0.1", "--port", "0"],
        cwd=runtime,
        env=_clean_environment(token, session_id),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        descriptor_line = process.stdout.readline() if process.stdout else ""
        descriptor = json.loads(descriptor_line)
        result: dict[str, object] | None = None
        for _ in range(20):
            try:
                result = _health(int(descriptor["port"]), token, session_id)
                break
            except OSError:
                if process.poll() is not None:
                    raise RuntimeError("staged sidecar exited before its health check") from None
                time.sleep(0.1)
        if result != {
            "protocol": {"major": 1, "minor": 0},
            "session_id": session_id,
            "sidecar_id": descriptor["sidecar_id"],
            "status": "healthy",
        }:
            raise RuntimeError(f"unexpected staged-sidecar health response: {result!r}")
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: smoke_live_sidecar_runtime.py <staged-python-directory>")
    main(Path(sys.argv[1]).resolve())
