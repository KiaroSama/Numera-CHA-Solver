"""Actual installed release OCR with checked CPU runtime and isolated CI-only model state."""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path


def checked_copy(source, destination, expected):
    if source.is_symlink() or any(parent.is_symlink() for parent in source.parents) or not source.is_file() or source.stat().st_size != expected["size"]:
        raise ValueError("Verified model input is missing, linked or changed in size.")
    digest = hashlib.sha256()
    with source.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    if digest.hexdigest() != expected["sha256"]:
        raise ValueError("Verified model input SHA-256 differs from the immutable manifest.")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with source.open("rb") as original, destination.open("xb") as output:
        shutil.copyfileobj(original, output, length=1024 * 1024)
    assert hashlib.sha256(destination.read_bytes()).hexdigest() == expected["sha256"]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--runtime-root", type=Path, required=True)
    parser.add_argument("--image", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    work = args.work.resolve()
    if work == root or work.is_relative_to(root) or args.work.is_symlink() or any(parent.is_symlink() for parent in args.work.parents):
        raise ValueError("Installed OCR model state must be isolated outside the delivered archive.")
    work.mkdir(parents=True, exist_ok=False)
    executable = root / "bin" / ("numera-cha.exe" if os.name == "nt" else "numera-cha")
    mcp = root / "bin" / ("numera-cha-solver-mcp.exe" if os.name == "nt" else "numera-cha-solver-mcp")
    env = os.environ.copy()
    env["NUMERA_ROOT"] = str(work)
    env["NUMERA_DEVICE"] = "cpu"
    library = "onnxruntime.dll" if os.name == "nt" else "libonnxruntime.dylib" if sys.platform == "darwin" else "libonnxruntime.so"
    env["NUMERA_ORT_LIBRARY"] = str(root / "data/runtime" / library)
    for key in ("NUMERA_WORKER_EXECUTABLE", "NUMERA_DLL_DIRECTORY", "NUMERA_API_TOKEN", "NUMERA_MCP_TOKEN", "NUMERA_FIXTURE_WORKER_EXECUTABLE"):
        env.pop(key, None)
    deadline = time.monotonic() + 55

    def run(binary, arguments, expected, maximum=10):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("Installed release smoke exceeded its shared deadline.")
        result = subprocess.run([str(binary), *arguments], cwd=work, env=env,
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            timeout=min(maximum, remaining), creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        assert result.returncode == expected, f"Installed operation exited {result.returncode}, expected {expected}."
        return result.stdout.decode("utf-8", errors="strict")

    success = False
    try:
        assert (root / "source" / "REVISION.txt").is_file()
        for relative in ("plugin.json", ".claude-plugin/plugin.json", ".codex-plugin/plugin.json", ".cursor-plugin/plugin.json"):
            metadata = json.loads((root / relative).read_text(encoding="utf-8"))
            assert metadata["name"] == "numera-cha-solver" and metadata["version"] == "0.3.0"
        for relative in ("skills/numera-cha-solver/SKILL.md", "assets/brand/logo.png", "docs/QUICKSTART.fa.md", "docs/TESTING.md", "reports/VALIDATION.md", "CONTEXT.md", "tests/fixtures/ocr/ascii-smoke.png"):
            assert (root / relative).is_file(), "Documented package payload is missing."
        if os.name == "nt":
            assert (root / "run.ps1").is_file()
        assert not list(root.rglob("*.onnx")), "Weights unexpectedly shipped."
        assert not list(root.rglob(".ci-oracle-env")), "Historic oracle environment unexpectedly shipped."
        result = json.loads(run(executable, ["--device", "cpu", "recognize", "--text", "7+5"], 0))
        assert result["answer"] == "12" and result["server_verified"] is False
        run(executable, ["--device", "invalid", "recognize", "--text", "7+5"], 2)
        run(mcp, ["--help"], 0)
        manifest = json.loads((root / "resources" / "models.json").read_text(encoding="utf-8"))
        model = next(item for item in manifest["models"] if item["id"] == "ppocr-v6-small")
        for asset in model["assets"]:
            if Path(asset["file"]).name != asset["file"]:
                raise ValueError("Model asset path is invalid.")
            relative = Path("data/models") / model["id"] / model["revision"] / asset["file"]
            checked_copy(args.runtime_root.resolve() / relative, work / relative, asset)
        # Resolve the packaged sibling worker and packaged CPU library; no CI worker override is allowed.
        result = json.loads(run(executable, ["--device", "cpu", "recognize", "--image", str(args.image.resolve())], 0, 45))
        assert result["answer"] == "NUMERA 123"
        assert result["device"] == "cpu" and result["server_verified"] is False
        assert result["model_revision"] == model["revision"]
        evidence = result["evidence"]
        assert evidence["input_shape"] == [1, 3, 48, 320]
        assert evidence["profile_node_counts"].get("CPUExecutionProvider", 0) > 0
        assert evidence["profile_node_counts"].get("CUDAExecutionProvider", 0) == 0
        logs = list((work / "logs").glob("*.log"))
        assert logs and all(" UTC] [" in path.read_text(encoding="utf-8") for path in logs)
        assert not list(root.rglob("*.onnx")), "CI smoke mutated delivered package content."
        success = True
        print("Installed release CLI/connector/real CPU OCR/profile/input-shape/source/log checks passed.")
    finally:
        if success:
            shutil.rmtree(work)  # Only this invocation's newly-created private smoke state.


if __name__ == "__main__":
    main()
