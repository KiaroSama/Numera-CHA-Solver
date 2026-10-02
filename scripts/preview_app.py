"""Manual local preview entry point; no default visible browser startup."""

import secrets
from dataclasses import replace
from pathlib import Path

import uvicorn

from numera_cha_solver.api import create_app
from numera_cha_solver.config import Settings
from numera_cha_solver.logging import close, initialize

ROOT = Path(__file__).resolve().parents[1]

if __name__ == "__main__":
    settings = Settings.from_env(root=ROOT)
    token = secrets.token_urlsafe(32)
    initialize(settings.log_dir, "preview-app")
    try:
        uvicorn.run(
            create_app(replace(settings, api_token=token)),
            host="127.0.0.1",
            port=8765,
            access_log=False,
            log_level="warning",
        )
    finally:
        close()
