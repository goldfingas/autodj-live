"""Authentication and dependency-boundary tests for the AutoDJ Live sidecar."""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
from uuid import UUID

import pytest
from fastapi.testclient import TestClient

from autodj.live_sidecar import create_live_sidecar_app

TOKEN = "ab" * 32
SESSION_ID = UUID("11111111-2222-3333-4444-555555555555")
AUTHORIZATION = {"Authorization": f"Bearer {TOKEN}"}
SESSION = {"X-AutoDJ-Live-Session": str(SESSION_ID)}
UNAUTHORIZED = {"detail": "Unauthorized"}


@pytest.fixture
def client() -> TestClient:
    app = create_live_sidecar_app(
        token_digest=hashlib.sha256(TOKEN.encode("ascii")).digest(),
        session_id=SESSION_ID,
    )
    return TestClient(app)


@pytest.mark.parametrize(
    "headers",
    [
        {},
        SESSION,
        {**SESSION, "Authorization": TOKEN},
        {**SESSION, "Authorization": "Basic " + TOKEN},
        {**SESSION, "Authorization": "Bearer"},
        {**SESSION, "Authorization": "Bearer " + "cd" * 32},
        {**AUTHORIZATION, "X-AutoDJ-Live-Session": str(UUID(int=0))},
        AUTHORIZATION,
    ],
    ids=[
        "missing-both",
        "missing-token",
        "missing-scheme",
        "wrong-scheme",
        "missing-value",
        "wrong-token",
        "stale-session",
        "missing-session",
    ],
)
def test_health_rejects_every_bad_credential_without_state(
    client: TestClient, headers: dict[str, str]
) -> None:
    response = client.get("/live/v1/health", headers=headers)

    assert response.status_code == 401
    assert response.json() == UNAUTHORIZED
    assert response.headers["Cache-Control"] == "no-store"


def test_health_returns_only_live_identity_for_matching_credentials(client: TestClient) -> None:
    response = client.get("/live/v1/health", headers={**AUTHORIZATION, **SESSION})

    assert response.status_code == 200
    body = response.json()
    assert body == {
        "protocol": {"major": 1, "minor": 0},
        "session_id": str(SESSION_ID),
        "sidecar_id": body["sidecar_id"],
        "status": "healthy",
    }
    UUID(body["sidecar_id"])
    assert response.headers["Cache-Control"] == "no-store"


def test_sidecar_registers_no_station_or_documentation_routes(client: TestClient) -> None:
    assert [route.path for route in client.app.routes] == ["/live/v1/health"]
    assert client.get("/api/stations", headers={**AUTHORIZATION, **SESSION}).status_code == 404


def test_importing_sidecar_does_not_import_audio_catalog_or_model_dependencies() -> None:
    script = """
import json
import sys
import autodj.live_sidecar

blocked = (
    "torch",
    "faiss",
    "sounddevice",
    "autodj.model",
    "autodj.stations",
    "autodj.player",
    "autodj.mixbus",
)
print(json.dumps([name for name in blocked if name in sys.modules]))
"""
    completed = subprocess.run(
        [sys.executable, "-c", script],
        check=True,
        capture_output=True,
        text=True,
    )

    assert json.loads(completed.stdout) == []


def test_importing_sidecar_cli_defers_server_stack_until_launch_secret_is_read() -> None:
    script = """
import json
import sys
import autodj.live_sidecar_cli

blocked = ("uvicorn", "fastapi", "starlette")
print(json.dumps([name for name in blocked if name in sys.modules]))
"""
    completed = subprocess.run(
        [sys.executable, "-c", script],
        check=True,
        capture_output=True,
        text=True,
    )

    assert json.loads(completed.stdout) == []
