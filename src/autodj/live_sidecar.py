"""Minimal authenticated HTTP surface for AutoDJ Live."""

from __future__ import annotations

import hashlib
import hmac
from uuid import UUID, uuid4

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse

PROTOCOL_VERSION = {"major": 1, "minor": 0}
_NO_STORE = {"Cache-Control": "no-store"}
_UNAUTHORIZED = {"detail": "Unauthorized"}


def create_live_sidecar_app(*, token_digest: bytes, session_id: UUID) -> FastAPI:
    """Create the isolated Live sidecar with one authenticated health route."""
    if len(token_digest) != hashlib.sha256().digest_size:
        raise ValueError("token_digest must be a SHA-256 digest")

    expected_token_digest = bytes(token_digest)
    expected_session_digest = hashlib.sha256(str(session_id).encode("ascii")).digest()
    sidecar_id = uuid4()
    app = FastAPI(docs_url=None, redoc_url=None, openapi_url=None)
    app.state.sidecar_id = sidecar_id

    @app.get("/live/v1/health")
    async def health(request: Request) -> JSONResponse:
        authorizations = request.headers.getlist("Authorization")
        sessions = request.headers.getlist("X-AutoDJ-Live-Session")
        authorization = authorizations[0] if len(authorizations) == 1 else ""
        supplied_session = sessions[0] if len(sessions) == 1 else ""
        scheme, separator, candidate = authorization.partition(" ")
        bearer = candidate if separator and scheme.casefold() == "bearer" else ""
        supplied_token_digest = hashlib.sha256(bearer.encode("utf-8")).digest()
        supplied_session_digest = hashlib.sha256(supplied_session.encode("utf-8")).digest()

        token_matches = hmac.compare_digest(supplied_token_digest, expected_token_digest)
        session_matches = hmac.compare_digest(
            supplied_session_digest, expected_session_digest
        )
        if not (token_matches and session_matches):
            return JSONResponse(status_code=401, content=_UNAUTHORIZED, headers=_NO_STORE)

        return JSONResponse(
            content={
                "protocol": PROTOCOL_VERSION,
                "session_id": str(session_id),
                "sidecar_id": str(sidecar_id),
                "status": "healthy",
            },
            headers=_NO_STORE,
        )

    return app
