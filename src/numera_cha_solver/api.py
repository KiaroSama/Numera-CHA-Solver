"""Loopback-only HTTP adapter with strict inputs and same-origin sessions."""

import base64
import binascii
import hmac
import secrets
from contextlib import asynccontextmanager
from importlib.resources import files
from typing import Literal
from uuid import uuid4

from fastapi import Depends, FastAPI, HTTPException, Request, Response
from fastapi.exceptions import RequestValidationError
from fastapi.responses import JSONResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, ConfigDict, Field

from . import __version__
from .config import Settings
from .core.contracts import Input, InputError
from .models import ModelRegistry
from .sdk import Numera
from .store import TaskStore
from .tasks import TaskService


class AssetRequest(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)
    kind: Literal["text", "image", "audio", "browser"]
    text: str = Field(default="", max_length=4096)
    asset_base64: str = Field(default="", max_length=11_184_812)
    subtype: str = Field(default="auto", max_length=64)
    allowed_characters: str = Field(default="", max_length=256)
    revision: str = Field(default="", max_length=128)

    def input(self) -> Input:
        try:
            content = base64.b64decode(self.asset_base64, validate=True)
        except (ValueError, binascii.Error):
            raise HTTPException(422, "Asset must be valid base64.") from None
        return Input(
            self.kind, self.text, content, self.subtype, self.allowed_characters, self.revision
        )


def create_app(settings: Settings | None = None) -> FastAPI:
    settings = settings or Settings.from_env()
    if not settings.api_token:
        raise ValueError("Set NUMERA_API_TOKEN or use the authenticated serve command.")
    import time

    sessions: dict[str, float] = {}

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        from .ownership import ServerOwnership

        owner = ServerOwnership(settings.data_dir / ".server-owner.lock")
        owner.acquire()
        sdk = None
        store = None
        service = None
        try:
            sdk = Numera(settings)
            store = TaskStore(settings.data_dir / "tasks.sqlite3", settings.result_ttl)
            service = TaskService(sdk, store)
            app.state.sdk, app.state.store, app.state.tasks = sdk, store, service
            await service.start()
            yield
        finally:
            try:
                if service is not None:
                    await service.close()
                else:
                    try:
                        if sdk is not None:
                            sdk.close()
                    finally:
                        if store is not None:
                            store.close()
            finally:
                sessions.clear()
                owner.close()

    app = FastAPI(
        title="Numera CHA-Solver",
        version=__version__,
        lifespan=lifespan,
        docs_url=None,
        redoc_url=None,
        openapi_url=None,
    )

    async def authorize(request: Request) -> None:
        header = request.headers.get("authorization", "")
        bearer = header[7:] if header.startswith("Bearer ") else ""
        session = request.cookies.get("numera_session", "")
        if (
            not (bearer and hmac.compare_digest(bearer, settings.api_token))
            and sessions.get(session, 0) <= time.monotonic()
        ):
            raise HTTPException(401, "Local client authorization is required.")

    @app.middleware("http")
    async def local_boundary(request: Request, call_next):
        request_id = str(uuid4())
        request.state.request_id = request_id
        host = request.headers.get("host", "").split(":")[0]
        if host not in {"127.0.0.1", "localhost", "testserver"}:
            return JSONResponse(
                {
                    "error": {"code": "invalid_host", "message": "Loopback host is required."},
                    "request_id": request_id,
                },
                403,
            )
        origin = request.headers.get("origin")
        if origin and origin != f"{request.url.scheme}://{request.headers.get('host')}":
            return JSONResponse(
                {
                    "error": {
                        "code": "invalid_origin",
                        "message": "Cross-origin requests are not permitted.",
                    },
                    "request_id": request_id,
                },
                403,
            )
        try:
            declared = int(request.headers.get("content-length", "0"))
        except ValueError:
            declared = -1
        if declared < 0 or declared > 11_190_000:
            return JSONResponse(
                {
                    "error": {
                        "code": "input_too_large",
                        "message": "Request body exceeds its limit.",
                    },
                    "request_id": request_id,
                },
                413,
            )
        if request.method in {"POST", "PUT", "PATCH"}:
            body = bytearray()
            import asyncio

            try:
                async with asyncio.timeout(10):
                    async for chunk in request.stream():
                        body.extend(chunk)
                        if len(body) > 11_190_000:
                            break
            except TimeoutError:
                return JSONResponse(
                    {
                        "error": {
                            "code": "input_timeout",
                            "message": "Request body deadline exhausted.",
                        }
                    },
                    408,
                )
            if len(body) > 11_190_000:
                return JSONResponse(
                    {
                        "error": {
                            "code": "input_too_large",
                            "message": "Request body exceeds its limit.",
                        }
                    },
                    413,
                )
            request._body = bytes(body)
        response = await call_next(request)
        response.headers["X-Request-ID"] = request_id
        response.headers["X-Content-Type-Options"] = "nosniff"
        response.headers["Content-Security-Policy"] = (
            "default-src 'self'; img-src 'self' data:; frame-ancestors 'none'; "
            "base-uri 'none'; form-action 'self'"
        )
        response.headers["Cache-Control"] = "no-store"
        return response

    @app.exception_handler(HTTPException)
    async def http_error(request: Request, exc: HTTPException):
        return JSONResponse(
            {
                "error": {"code": f"http_{exc.status_code}", "message": str(exc.detail)},
                "request_id": request.state.request_id,
            },
            exc.status_code,
        )

    @app.exception_handler(InputError)
    async def observation_error(request: Request, exc: InputError):
        return JSONResponse(
            {
                "error": {"code": exc.code, "message": str(exc)},
                "request_id": request.state.request_id,
            },
            422,
        )

    @app.exception_handler(RequestValidationError)
    async def input_error(request: Request, exc: RequestValidationError):
        return JSONResponse(
            {
                "error": {
                    "code": "invalid_input",
                    "message": "Request does not match the strict input schema.",
                },
                "request_id": request.state.request_id,
            },
            422,
        )

    @app.get("/health/live")
    async def live():
        return {"live": True, "version": __version__}

    @app.get("/health/ready")
    async def ready():
        status = ModelRegistry(settings).verify("ppocr-v6-small")
        return {
            "ready": True,
            "model_assets_ready": status["availability"] == "ready",
            "arithmetic_ready": True,
            "model": status,
            "gpu_execution_verified": False,
        }

    @app.post("/v1/session", dependencies=[Depends(authorize)])
    async def login(response: Response):
        if len(sessions) >= 16:
            sessions.clear()
        session = secrets.token_urlsafe(32)
        sessions[session] = time.monotonic() + 3600
        response.set_cookie(
            "numera_session", session, httponly=True, samesite="strict", max_age=3600
        )
        return {"authenticated": True}

    @app.get("/v1/capabilities", dependencies=[Depends(authorize)])
    async def inventory():
        return await app.state.sdk.capabilities()

    @app.get("/v1/models", dependencies=[Depends(authorize)])
    async def models():
        return {"models": await app.state.sdk.models(), "device": settings.device}

    @app.post("/v1/detect", dependencies=[Depends(authorize)])
    async def detection(payload: AssetRequest):
        return await app.state.sdk.detect(payload.input())

    @app.post("/v1/recognize", dependencies=[Depends(authorize)])
    async def recognize(payload: AssetRequest):
        return (await app.state.sdk.recognize(payload.input())).to_dict()

    @app.post("/v1/tasks", status_code=202, dependencies=[Depends(authorize)])
    async def create_task(payload: AssetRequest, request: Request):
        key = request.headers.get("idempotency-key", "")
        if not key:
            raise HTTPException(400, "Idempotency-Key is required.")
        try:
            return await app.state.tasks.create(key, payload.input())
        except ValueError as exc:
            raise HTTPException(409, str(exc)) from None

    @app.get("/v1/tasks", dependencies=[Depends(authorize)])
    async def task_list():
        return {"tasks": app.state.store.list()}

    @app.get("/v1/tasks/{identifier}", dependencies=[Depends(authorize)])
    async def task(identifier: str):
        try:
            return app.state.store.get(identifier)
        except KeyError:
            raise HTTPException(404, "Task is missing or expired.") from None

    @app.post("/v1/tasks/{identifier}/cancel", dependencies=[Depends(authorize)])
    async def cancel(identifier: str):
        try:
            return await app.state.tasks.cancel(identifier)
        except KeyError:
            raise HTTPException(404, "Task is missing or expired.") from None

    app.mount(
        "/",
        StaticFiles(directory=str(files("numera_cha_solver").joinpath("resources/web")), html=True),
        name="dashboard",
    )
    return app
