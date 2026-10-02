"""JSON output on stdout; diagnostics and secrets stay off stdout."""

import argparse
import asyncio
import json
import os
import secrets
from pathlib import Path

from .assets import read_asset
from .benchmark import benchmark
from .config import Settings
from .core.contracts import Input, InputError
from .logging import close, initialize
from .models import ModelRegistry
from .sdk import Numera


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(
        prog="numera-cha", description="Local perception and owned verification laboratory."
    )
    result.add_argument(
        "--device", choices=["cuda", "cpu"], default=os.environ.get("NUMERA_DEVICE", "cuda")
    )
    result.add_argument("--debug", action="store_true")
    commands = result.add_subparsers(dest="command", required=True)
    diagnosis = commands.add_parser(
        "doctor", help="Inspect prerequisites; inventory is not inference proof."
    )
    diagnosis.add_argument(
        "--smoke", type=Path, help="Licensed local image for real inference evidence."
    )
    commands.add_parser("capabilities", help="List declared coverage, evidence and blockers.")
    models = commands.add_parser("models", help="Reviewed immutable model assets.")
    models.add_argument("operation", choices=["list", "verify", "fetch"])
    models.add_argument("model", nargs="?", default="ppocr-v6-small")
    for name in ("detect", "recognize", "solve"):
        command = commands.add_parser(
            name, help="Process local data; no third-party browser bypass."
        )
        group = command.add_mutually_exclusive_group(required=True)
        group.add_argument("--text")
        group.add_argument("--image", type=Path)
        command.add_argument("--characters", default="")
    serve = commands.add_parser("serve", help="Start authenticated loopback API/dashboard.")
    serve.add_argument("--host", default="127.0.0.1", choices=["127.0.0.1", "localhost"])
    serve.add_argument("--port", type=int, default=8765)
    evaluate = commands.add_parser("benchmark", help="Evaluate a licensed labeled local manifest.")
    evaluate.add_argument("manifest", type=Path)
    commands.add_parser("report", help="Coverage summary with no inferred support claims.")
    return result


async def execute(args: argparse.Namespace, settings: Settings) -> tuple[dict, int]:
    sdk = Numera(settings)
    try:
        if args.command in {"capabilities", "report"}:
            return await sdk.capabilities(), 0
        if args.command == "models":
            registry = ModelRegistry(settings)
            if args.operation == "list":
                return {"models": registry.list()}, 0
            status = await asyncio.to_thread(getattr(registry, args.operation), args.model)
            return status, 0 if status["availability"] == "ready" else 3
        if args.command == "doctor":
            status = await sdk.doctor()
            if args.smoke:
                result = await sdk.recognize(
                    {"kind": "image", "asset": read_asset(args.smoke, settings)}
                )
                status["smoke"] = result.to_dict()
                status["gpu_execution_verified"] = result.device == "cuda" and bool(
                    result.evidence.get("profile_node_counts", {}).get("CUDAExecutionProvider")
                )
                return status, 0 if result.outcome == "recognized" else 3
            return status, 0
        if args.command == "benchmark":
            return await benchmark(sdk, args.manifest), 0
        request = (
            Input("text", text=args.text)
            if args.text is not None
            else Input(
                "image", asset=read_asset(args.image, settings), allowed_characters=args.characters
            )
        )
        if args.command == "detect":
            return await sdk.detect(request), 0
        result = await getattr(sdk, args.command)(request)
        return result.to_dict(), 0 if result.outcome == "recognized" else 3 if result.outcome in {
            "unsupported",
            "needs_user_action",
        } else 4
    finally:
        sdk.close()


def main() -> int:
    args = parser().parse_args()
    settings = Settings.from_env(device=args.device)
    logger = initialize(settings.log_dir, debug=args.debug)
    code = 0
    try:
        if args.device == "cpu":
            logger.warning("gpu explicit CPU inference selected")
        if args.command == "serve":
            if not 1 <= args.port <= 65535:
                raise InputError("invalid_config", "Port must be between 1 and 65535.")
            token = settings.api_token
            if not token:
                token = secrets.token_urlsafe(32)
                settings.data_dir.mkdir(parents=True, exist_ok=True)
                path = settings.data_dir / "api-token.txt"
                descriptor = os.open(path, os.O_CREAT | os.O_TRUNC | os.O_WRONLY, 0o600)
                with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
                    stream.write(token)
                if os.name == "nt":
                    import subprocess

                    sid = subprocess.run(
                        ["whoami", "/user", "/fo", "csv", "/nh"],
                        capture_output=True,
                        text=True,
                        encoding="utf-8",
                        timeout=5,
                        creationflags=subprocess.CREATE_NO_WINDOW,
                    )
                    import csv

                    identity = next(csv.reader([sid.stdout.strip()]))[1]
                    permissions = subprocess.run(
                        ["icacls", str(path), "/inheritance:r", "/grant:r", f"*{identity}:F"],
                        capture_output=True,
                        timeout=5,
                        creationflags=subprocess.CREATE_NO_WINDOW,
                    )
                    if permissions.returncode != 0:
                        path.unlink(missing_ok=True)
                        raise InputError(
                            "credential_permissions",
                            "Local credential ACL could not be restricted.",
                        )
                logger.info(
                    "config local API credential written to restricted data file; value not printed"
                )
            from dataclasses import replace

            import uvicorn

            from .api import create_app

            uvicorn.run(
                create_app(replace(settings, api_token=token)),
                host=args.host,
                port=args.port,
                log_level="warning",
                access_log=False,
            )
        else:
            response, code = asyncio.run(execute(args, settings))
            print(json.dumps(response, ensure_ascii=False, indent=2))
    except (InputError, ValueError, OSError) as exc:
        code = 2
        message = (
            str(exc)
            if isinstance(exc, InputError)
            else "Local operation failed; check input paths and configuration."
        )
        print(
            json.dumps(
                {
                    "outcome": "failed",
                    "error": {"code": getattr(exc, "code", "invalid_input"), "message": message},
                }
            )
        )
        logger.error("operation failed code=%s", getattr(exc, "code", "invalid_input"))
    except KeyboardInterrupt:
        code = 130
        logger.warning("cleanup cancellation requested")
    finally:
        logger.info("cleanup shutdown exit_code=%s", code)
        close()
    return code


if __name__ == "__main__":
    raise SystemExit(main())
