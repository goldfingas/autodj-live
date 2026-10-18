"""Private subprocess runner for the AutoDJ Live Python sidecar."""

from __future__ import annotations

import hashlib
import json
import os
import socket
import sys
from uuid import UUID


class LiveSidecarStartupError(RuntimeError):
    """A secret-free launch configuration error."""


def _token_digest_from_environment() -> bytes:
    source = os.environ.pop("AUTODJ_LIVE_TOKEN", None)
    if (
        source is None
        or len(source) != 64
        or any(character not in "0123456789abcdef" for character in source)
    ):
        source = None
        raise LiveSidecarStartupError(
            "AUTODJ_LIVE_TOKEN must be 64 lowercase hex characters"
        )

    raw_token = bytearray(source, "ascii")
    source = None
    try:
        return hashlib.sha256(raw_token).digest()
    finally:
        raw_token[:] = b"\x00" * len(raw_token)


def _session_from_environment() -> UUID:
    value = os.environ.pop("AUTODJ_LIVE_SESSION_ID", None)
    if value is None:
        raise LiveSidecarStartupError("AUTODJ_LIVE_SESSION_ID must be a UUID")
    try:
        return UUID(value)
    except ValueError:
        raise LiveSidecarStartupError("AUTODJ_LIVE_SESSION_ID must be a UUID") from None


def run_live_sidecar(*, host: str, port: int) -> None:
    """Bind an ephemeral loopback socket, emit its descriptor, and serve it."""
    if host != "127.0.0.1" or port != 0:
        raise LiveSidecarStartupError(
            "the Live sidecar requires --host 127.0.0.1 and --port 0"
        )

    token_digest = _token_digest_from_environment()
    session_id = _session_from_environment()

    import uvicorn

    from autodj.live_sidecar import PROTOCOL_VERSION, create_live_sidecar_app

    app = create_live_sidecar_app(token_digest=token_digest, session_id=session_id)

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind((host, port))
        bound_port = listener.getsockname()[1]
        descriptor = {
            "protocol": PROTOCOL_VERSION,
            "port": bound_port,
            "pid": os.getpid(),
            "sidecar_id": str(app.state.sidecar_id),
        }
        json.dump(descriptor, sys.stdout, separators=(",", ":"))
        sys.stdout.write("\n")
        sys.stdout.flush()

        config = uvicorn.Config(app, host=host, port=bound_port, access_log=False)
        uvicorn.Server(config).run(sockets=[listener])
