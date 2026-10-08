"""Exercise both runtimes against copied fixture state, never the owner's task database."""

import argparse
import asyncio
import hashlib
import json
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE_ROOT = Path(os.environ.get("NUMERA_ORACLE_SOURCE", ROOT)).resolve()
sys.path.insert(0, str(SOURCE_ROOT / "src"))
from numera_cha_solver.config import Settings  # noqa: E402
from numera_cha_solver.core.contracts import Input  # noqa: E402
from numera_cha_solver.logging import close, initialize  # noqa: E402
from numera_cha_solver.sdk import Numera  # noqa: E402
from numera_cha_solver.store import TaskStore  # noqa: E402


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    args = parser.parse_args()
    work = args.work.resolve()
    if not work.is_relative_to(ROOT) or work == ROOT or work == ROOT / "data":
        raise ValueError("Rollback fixture must be isolated under the project working directory.")
    work.mkdir(parents=True, exist_ok=True)
    logger = initialize(work / "logs")
    sdk = Numera(Settings(root=work))
    try:
        if SOURCE_ROOT == ROOT:
            pin = subprocess.run(["git", "diff", "--quiet", "a8e25292317621e900d908c56927c489eecae06a", "--", "src/numera_cha_solver", "pyproject.toml", "uv.lock"], cwd=ROOT, capture_output=True, timeout=5, creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
            if pin.returncode != 0:
                raise RuntimeError("Rollback source differs from its pinned revision.")
        source = asyncio.run(sdk.recognize({"kind": "text", "text": "7+5"})).to_dict()
        request = Input("text", text="7+5")
        encoded = json.dumps({"kind": request.kind, "text": request.text,
            "asset_sha256": hashlib.sha256(request.asset).hexdigest(), "subtype": request.subtype,
            "allowed_characters": request.allowed_characters, "revision": request.revision}, sort_keys=True)
        digest = hashlib.sha256(encoded.encode("utf-8")).hexdigest()
        database = work / "data/tasks.sqlite3"
        with TaskStore(database, 60) as store:
            task = store.create("rollback-fixture", digest, 120)
            assert store.finish(task["id"], source)
        env = os.environ.copy()
        env["NUMERA_ROOT"] = str(work)
        env.pop("NUMERA_API_TOKEN", None)
        process = subprocess.run([str(args.executable.resolve()), "recognize", "--text", "7+5"],
            env=env, cwd=work, capture_output=True, timeout=10,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        assert process.returncode == 0
        target = json.loads(process.stdout.decode("utf-8"))
        assert target == source
        # Both runtimes write/read the same copied WAL/schema; native HTTP results must survive rollback.
        import socket
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        env["NUMERA_API_TOKEN"] = "rollback-owned-test-only"
        child = subprocess.Popen([str(args.executable.resolve()), "serve", "--port", str(port)],
            env=env, cwd=work, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        try:
            import urllib.request
            deadline = time.monotonic() + 5
            while True:
                if child.poll() is not None:
                    raise RuntimeError("Native rollback fixture server exited before readiness.")
                try:
                    with urllib.request.urlopen(f"http://127.0.0.1:{port}/health/live", timeout=0.5) as response:
                        assert json.loads(response.read().decode("utf-8"))["live"] is True
                    break
                except OSError:
                    if time.monotonic() >= deadline:
                        raise TimeoutError("Native rollback fixture server did not become ready.") from None
                    time.sleep(0.02)
            def native_request(path, payload=None, key=None):
                headers = {"Authorization": "Bearer rollback-owned-test-only"}
                data = None
                if payload is not None:
                    headers["Content-Type"] = "application/json"
                    data = json.dumps(payload).encode("utf-8")
                if key:
                    headers["Idempotency-Key"] = key
                request = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=data, headers=headers)
                with urllib.request.urlopen(request, timeout=2) as response:
                    return json.loads(response.read().decode("utf-8"))
            native_task = native_request("/v1/tasks", {"kind": "text", "text": "7+5"}, "native-rollback-fixture")
            deadline = time.monotonic() + 5
            while native_task["state"] != "terminal":
                if time.monotonic() >= deadline:
                    raise TimeoutError("Native durable result deadline exhausted.")
                native_task = native_request(f"/v1/tasks/{native_task['id']}")
            assert native_task["result"] == source
            assert native_request("/v1/tasks", {"kind": "text", "text": "7+5"}, "native-rollback-fixture")["id"] == native_task["id"]
        finally:
            if child.poll() is None:
                child.terminate()
            child.wait(timeout=5)
        import sqlite3
        with sqlite3.connect(database) as fixture:
            now = time.time()
            fixture.execute("INSERT INTO tasks VALUES(?,?,?,?,?,?,?,NULL,NULL,?)", ("copied-unfinished", "copied-unfinished-key", digest, now, now + 120, now + 60, "recognizing", '[{"state":"queued"},{"state":"recognizing"}]'))
        with TaskStore(database, 60) as restored:
            assert restored.get("copied-unfinished")["result"]["error"]["code"] == "interrupted"
            result = restored.get(task["id"])
            assert result["result"] == source
            assert restored.create("rollback-fixture", digest, 120)["id"] == task["id"]
            native_result = restored.get(native_task["id"])
            assert native_result["result"] == source
            assert restored.create("native-rollback-fixture", digest, 120)["id"] == native_task["id"]
        logger.info("rollback source/native arithmetic and copied persisted state passed")
        print("Copied-state source/native rollback verified.")
        return 0
    finally:
        sdk.close()
        logger.info("cleanup rollback owned resources closed")
        close()


if __name__ == "__main__":
    raise SystemExit(main())
