"""One startup lease precedes socket binding and credential publication."""

import csv
import logging
import os
import secrets
import socket
import subprocess
import tempfile
from dataclasses import replace
from pathlib import Path

import uvicorn

from .config import Settings
from .core.contracts import InputError
from .ownership import ServerOwnership


def publish_credential(path: Path, token: str) -> None:
    temporary = None
    try:
        descriptor, name = tempfile.mkstemp(dir=path.parent, prefix="credential-")
        temporary = Path(name)
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            stream.write(token)
            stream.flush()
            os.fsync(stream.fileno())
        if os.name == "nt":
            sid = subprocess.run(
                ["whoami", "/user", "/fo", "csv", "/nh"],
                capture_output=True,
                text=True,
                encoding="utf-8",
                timeout=5,
                creationflags=subprocess.CREATE_NO_WINDOW,
            )
            try:
                identity = next(csv.reader([sid.stdout.strip()]))[1]
            except (IndexError, StopIteration):
                raise InputError(
                    "credential_permissions", "Owner identity could not be resolved."
                ) from None
            permissions = subprocess.run(
                ["icacls", str(temporary), "/inheritance:r", "/grant:r", f"*{identity}:F"],
                capture_output=True,
                timeout=5,
                creationflags=subprocess.CREATE_NO_WINDOW,
            )
            if sid.returncode or permissions.returncode:
                raise InputError(
                    "credential_permissions", "Local credential ACL could not be restricted."
                )
        temporary.replace(path)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def serve(settings: Settings, host: str, port: int) -> None:
    from .api import create_app

    if not 1 <= port <= 65535:
        raise InputError("invalid_config", "Port must be between 1 and 65535.")
    owner = ServerOwnership(settings.data_dir / ".server-owner.lock")
    try:
        owner.acquire()
    except RuntimeError:
        raise InputError("server_busy", "Another server owns this local task store.") from None
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
            if os.name == "nt":
                listener.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
            listener.bind((host, port))
            listener.listen(128)
            token = settings.api_token or secrets.token_urlsafe(32)

            def ready() -> None:
                if not settings.api_token:
                    publish_credential(settings.data_dir / "api-token.txt", token)
                    logging.getLogger("numera").info(
                        "config local API credential published; value not printed"
                    )

            app = create_app(replace(settings, api_token=token), ownership=owner, on_ready=ready)
            server = uvicorn.Server(
                uvicorn.Config(app, host=host, port=port, log_level="warning", access_log=False)
            )
            server.run(sockets=[listener])
            if not server.started:
                raise InputError("startup_failed", "Local server startup did not complete.")
    finally:
        owner.close()
