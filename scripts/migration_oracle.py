"""Generate non-sensitive source expectations; never translate expected answers by hand."""

import argparse
import ast
import hashlib
import json
import platform
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))

from numera_cha_solver.core.arithmetic import evaluate  # noqa: E402
from numera_cha_solver.core.contracts import InputError  # noqa: E402
from numera_cha_solver.logging import close, initialize  # noqa: E402


def arithmetic_cases():
    inputs = [
        "12 * 3 = ?", "7+5", "-12/-3", "12/-3", "1- -2", "-0 + 0", "000001+2",
        " 1 + 2 ", "\x1c1+2\x1f", "1+2=", "1+2=?", "2 ** 100",
        "__import__('os')", "9/0", "3/2", "99999999+1", "100001+0", "100000+1",
        "-100000-1", "1+2?", "١+٢", "1.0+2", "", "1+2\n",
    ]
    cases = []
    for text in inputs:
        try:
            expected = {"answer": evaluate(text), "error": None}
        except InputError as exc:
            expected = {"answer": None, "error": {"code": exc.code, "message": str(exc)}}
        cases.append({"name": f"arithmetic-{len(cases):02d}", "input": text, "expected": expected})
    return cases


def source_inventory():
    rows = []
    for path in sorted((ROOT / "tests").glob("test_*.py")):
        content = path.read_bytes().replace(b"\r\n", b"\n")
        tree = ast.parse(content.decode("utf-8"))
        for node in tree.body:
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name.startswith("test_"):
                rows.append({
                    "source": path.relative_to(ROOT).as_posix(), "name": node.name,
                    "source_sha256": hashlib.sha256(content).hexdigest(), "source_hash_normalization": "CRLF to LF only",
                    "target": None, "status": "pending",
                })
    if not rows:
        raise RuntimeError("Source test inventory must not be empty.")
    return rows


def contract_cases():
    from dataclasses import asdict
    from numera_cha_solver.core.geometry import Transform
    from numera_cha_solver.verification import VerificationContext, validate_assessment
    from numera_cha_solver.engines.ocr_worker import decode
    import numpy as np
    expected = VerificationContext("turnstile", "localhost", "test", "session-a", True)
    valid = {"provider": "turnstile", "hostname": "localhost", "action": "test", "session_id": "session-a", "success": True}
    verification = []
    for test_keys in (True, False):
        context = VerificationContext(expected.provider, expected.hostname, expected.action, expected.session_id, test_keys)
        for assessment in (valid, {"token": "not-evidence"}, {**valid, "success": "true"}, *({**valid, field: "other"} for field in ("provider", "hostname", "action", "session_id"))):
            verification.append({"context": asdict(context), "input": assessment, "expected": validate_assessment(assessment, context).to_dict()})
    geometry = []
    for dimensions, operation, point in [([300, 300, 1.5, 2, 17, 80], "to_viewport", [100, 150]), ([300, 300, 1.5, 2, 17, 80], "to_image", [167, 380]), ([300, 300, 1, 1, 0, 0], "to_viewport", [300, 0]), ([0, 300, 1, 1, 0, 0], "to_image", [0, 0])]:
        try:
            answer = list(getattr(Transform(*dimensions), operation)(*point))
            result = {"answer": answer, "error": None}
        except InputError as error:
            result = {"answer": None, "error": {"code": error.code, "message": str(error)}}
        geometry.append({"dimensions": dimensions, "operation": operation, "point": point, "expected": result})
    ctc = []
    for rows in ([[.9, .05, .05]], [[.1, .8, .1], [.1, .8, .1], [.9, .05, .05], [.1, .8, .1], [.1, .1, .8]], [[.5, .5, 0]], [[-.1, .8, .3]], [[1.0001, 0, 0]]):
        output = np.asarray([rows], dtype=np.float32)
        try:
            text, confidence = decode(output, ["", "a", "b"])
            result = {"answer": text, "confidence": confidence, "error": None}
        except InputError as error:
            result = {"answer": None, "error": {"code": error.code, "message": str(error)}}
        ctc.append({"rows": rows, "expected": result})
    return {"verification": verification, "geometry": geometry, "ctc": ctc}


def http_cases(output):
    import base64
    from tempfile import TemporaryDirectory
    from fastapi.testclient import TestClient
    from numera_cha_solver.api import create_app
    from numera_cha_solver.config import Settings

    cases = [
        {"name": "unauthorized", "method": "GET", "path": "/v1/models"},
        {"name": "arithmetic", "method": "POST", "path": "/v1/recognize", "authorized": True, "body": {"kind": "text", "text": "1+2"}},
        {"name": "unknown-field", "method": "POST", "path": "/v1/recognize", "authorized": True, "body": {"kind": "text", "endpoint": "https://example.com"}},
        {"name": "invalid-kind", "method": "POST", "path": "/v1/recognize", "authorized": True, "body": {"kind": "unknown"}},
        {"name": "text-type", "method": "POST", "path": "/v1/recognize", "authorized": True, "body": {"kind": "text", "text": 3}},
        {"name": "text-bound", "method": "POST", "path": "/v1/recognize", "authorized": True, "text_count": 4097},
        {"name": "base64", "method": "POST", "path": "/v1/recognize", "authorized": True, "body": {"kind": "image", "asset_base64": "%%%"}},
        {"name": "large-valid-body", "method": "POST", "path": "/v1/recognize", "authorized": True, "asset_bytes": 1_600_000},
        {"name": "host", "method": "GET", "path": "/health/live", "headers": {"host": "outside.invalid"}},
        {"name": "origin", "method": "GET", "path": "/health/live", "headers": {"origin": "https://outside.invalid"}},
        {"name": "declared-bound", "method": "POST", "path": "/v1/recognize", "headers": {"content-length": "11190001"}},
        {"name": "missing-key", "method": "POST", "path": "/v1/tasks", "authorized": True, "body": {"kind": "text", "text": "1+2"}},
        {"name": "missing-task", "method": "GET", "path": "/v1/tasks/missing", "authorized": True},
    ]
    with TemporaryDirectory(dir=output, prefix="http-source-") as root:
        with TestClient(create_app(Settings(root=Path(root), api_token="owned-oracle-only"))) as client:
            for case in cases:
                headers = dict(case.get("headers", {}))
                if case.get("authorized"):
                    headers["authorization"] = "Bearer owned-oracle-only"
                body = case.get("body")
                if "text_count" in case:
                    body = {"kind": "text", "text": "é" * case["text_count"]}
                if "asset_bytes" in case:
                    body = {"kind": "audio", "asset_base64": base64.b64encode(bytes(case["asset_bytes"])).decode("ascii")}
                response = client.request(case["method"], case["path"], headers=headers, json=body)
                value = response.json()
                if "request_id" in value:
                    value["request_id"] = "<request-id>"
                observed = {key: response.headers[key] for key in ("x-content-type-options", "content-security-policy", "cache-control", "x-request-id") if key in response.headers}
                if "x-request-id" in observed:
                    observed["x-request-id"] = "<request-id>"
                case["expected"] = {"status": response.status_code, "body": value, "headers": observed}
    return {"normalization": ["request_id", "x-request-id"], "cases": cases}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    logger = initialize(ROOT / "logs")
    code = 0
    try:
        import subprocess
        source_revision = "a8e25292317621e900d908c56927c489eecae06a"
        verified = subprocess.run(["git", "diff", "--quiet", source_revision, "--", "src/numera_cha_solver", "pyproject.toml", "uv.lock", ".python-version", "tests/fixtures/ocr/ascii-smoke.png"], cwd=ROOT, capture_output=True, timeout=5, creationflags=subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0)
        if verified.returncode != 0:
            raise RuntimeError("Source oracle differs from its pinned revision; generation is blocked.")
        args.output.mkdir(parents=True, exist_ok=True)
        document = {
            "source_revision": "a8e25292317621e900d908c56927c489eecae06a",
            "python": platform.python_version(),
            "contract": "exact arithmetic answer and stable error",
            "cases": arithmetic_cases(),
        }
        from numera_cha_solver.config import Settings
        from numera_cha_solver.engines.ocr_worker import preprocess
        import numpy as np
        source_image = ROOT / "tests/fixtures/ocr/ascii-smoke.png"
        tensor = preprocess(source_image.read_bytes(), Settings(root=ROOT))
        (args.output / "ascii-preprocess.f32").write_bytes(tensor.astype("<f4").tobytes())
        (args.output / "preprocess.json").write_text(json.dumps({"shape": list(tensor.shape), "sha256": hashlib.sha256(tensor.astype("<f4").tobytes()).hexdigest(), "source_asset": "../ocr/ascii-smoke.png", "python": platform.python_version(), "numpy": np.__version__}, indent=2) + "\n", encoding="utf-8")
        import asyncio
        from numera_cha_solver.sdk import Numera
        values = [
            {"kind": "text", "text": "12 * 3 = ?"},
            {"kind": "text", "text": "3/2"},
            {"kind": "text", "text": "1+2", "subtype": "audio_digits"},
            {"kind": "audio"}, {"kind": "browser"},
            {"kind": "text", "text": 3}, {"kind": "unknown"}, {"kind": []},
            {"kind": "image", "asset": "https://example.com/image"},
            {"kind": "text", "endpoint": "https://example.com"}, None, [],
        ]
        async def sdk_cases():
            sdk = Numera(Settings(root=args.output / "empty-runtime"))
            try:
                return [{"name": f"sdk-{i:02d}", "input": value, "expected": (await sdk.recognize(value)).to_dict()} for i, value in enumerate(values)]
            finally:
                sdk.close()
        (args.output / "sdk.json").write_text(json.dumps({"cases": asyncio.run(sdk_cases())}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        (args.output / "contracts.json").write_text(json.dumps(contract_cases(), ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        (args.output / "http.json").write_text(json.dumps(http_cases(args.output), ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        from tempfile import TemporaryDirectory
        from numera_cha_solver.core.contracts import Input
        from numera_cha_solver.store import TaskStore
        from numera_cha_solver.tasks import TaskService
        async def digest_cases():
            with TemporaryDirectory(dir=args.output, prefix="digest-source-") as directory:
                settings = Settings(root=Path(directory))
                sdk = Numera(settings)
                store = TaskStore(settings.data_dir / "tasks.sqlite3", settings.result_ttl)
                service = TaskService(sdk, store)
                rows = []
                try:
                    for index, text in enumerate(["1+2", "\x7f", "\x80", "😀", "\x1f", '"\\\\']):
                        value = {"kind": "text", "text": text}
                        task = await service.create(f"digest-{index}", Input.from_dict(value))
                        digest = store.connection.execute("SELECT digest FROM tasks WHERE id=?", (task["id"],)).fetchone()[0]
                        rows.append({"input": value, "expected": digest})
                finally:
                    await service.close()
                return {"cases": rows}
        (args.output / "request-digests.json").write_text(json.dumps(asyncio.run(digest_cases()), ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        for name, value in (("arithmetic.json", document), ("source-tests.json", source_inventory())):
            (args.output / name).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        logger.info("oracle generated arithmetic_cases=%s source_tests=%s", len(document["cases"]), len(source_inventory()))
        print(f"Generated {len(document['cases'])} arithmetic oracle cases.")
    except (OSError, ValueError, RuntimeError):
        code = 2
        logger.exception("oracle fixture generation failed")
    finally:
        logger.info("cleanup oracle shutdown exit_code=%s", code)
        close()
    return code


if __name__ == "__main__":
    raise SystemExit(main())
