"""One owned process caches one reviewed model and reports actual execution."""

import json
import math
import os
from multiprocessing.connection import Connection
from pathlib import Path
from typing import Any

import numpy as np
from PIL import Image

from ..assets import image
from ..config import Settings
from ..core.contracts import InputError
from ..models import ModelRegistry


def preprocess(content: bytes, settings: Settings) -> np.ndarray:
    source = image(content, settings)
    ratio = source.width / source.height
    width = max(320, math.ceil(48 * ratio))
    if width > 2048:
        raise InputError("invalid_asset", "Text-line aspect ratio exceeds the model width budget.")
    resized_width = min(width, max(1, math.ceil(48 * ratio)))
    resized = source.resize((resized_width, 48), Image.Resampling.BILINEAR)
    pixels = np.asarray(resized, dtype=np.float32)[:, :, ::-1].transpose(2, 0, 1)
    normalized = (pixels / 255.0 - 0.5) / 0.5
    batch = np.zeros((1, 3, 48, width), dtype=np.float32)
    batch[0, :, :, :resized_width] = normalized
    return batch


def decode(output: np.ndarray, characters: list[str]) -> tuple[str, float]:
    if output.ndim != 3 or output.shape[0] != 1 or output.shape[2] != len(characters):
        raise InputError("model_contract", "OCR output does not match its reviewed character map.")
    if not np.isfinite(output).all() or output.min() < 0 or output.max() > 1.00001:
        raise InputError("model_contract", "OCR output contains invalid probability values.")
    indices = output[0].argmax(axis=1)
    confidence = output[0].max(axis=1)
    selected = [
        i for i, index in enumerate(indices) if index != 0 and (i == 0 or index != indices[i - 1])
    ]
    return "".join(characters[int(indices[i])] for i in selected), float(
        confidence[selected].mean()
    ) if selected else 0.0


def worker(connection: Connection, settings: Settings) -> None:
    dll_handle = None
    session = None
    profile_path = None
    try:
        import onnxruntime as ort

        if settings.dll_directory:
            directory = Path(settings.dll_directory).resolve(strict=True)
            if os.name == "nt":
                dll_handle = os.add_dll_directory(str(directory))
            ort.preload_dlls(directory=str(directory))
        else:
            ort.preload_dlls()
        registry = ModelRegistry(settings)
        status = registry.verify("ppocr-v6-small")
        if status["availability"] != "ready":
            raise InputError(
                status["availability"], "Reviewed model assets are missing or corrupt."
            )
        directory = registry.directory("ppocr-v6-small")
        characters = (
            [""] + (directory / "characters.txt").read_text(encoding="utf-8").splitlines() + [" "]
        )
        options = ort.SessionOptions()
        options.intra_op_num_threads = 1
        options.inter_op_num_threads = 1
        options.log_severity_level = 3
        options.enable_profiling = True
        profile_dir = settings.data_dir / "runtime"
        profile_dir.mkdir(parents=True, exist_ok=True)
        options.profile_file_prefix = str(profile_dir / f"ocr-{os.getpid()}")
        providers: list[Any] = ["CPUExecutionProvider"]
        if settings.device == "cuda":
            if "CUDAExecutionProvider" not in ort.get_available_providers():
                raise InputError(
                    "gpu_unavailable",
                    "CUDA provider is unavailable; CPU mode must be selected explicitly.",
                )
            providers = [
                (
                    "CUDAExecutionProvider",
                    {
                        "device_id": 0,
                        "gpu_mem_limit": 1073741824,
                        "cudnn_conv_algo_search": "HEURISTIC",
                    },
                ),
                "CPUExecutionProvider",
            ]
        session = ort.InferenceSession(
            str(directory / "inference.onnx"), sess_options=options, providers=providers
        )
        session.disable_fallback()
        active = session.get_providers()
        if settings.device == "cuda" and "CUDAExecutionProvider" not in active:
            raise InputError(
                "gpu_unavailable",
                "CUDA initialization fell back; strict GPU mode refuses CPU execution.",
            )
        inputs = session.get_inputs()
        outputs = session.get_outputs()
        if (
            len(inputs) != 1
            or len(outputs) != 1
            or inputs[0].type != "tensor(float)"
            or inputs[0].shape[1:3] != [3, 48]
        ):
            raise InputError(
                "model_contract",
                "ONNX input/output metadata does not match the reviewed text-line model.",
            )
        execution_evidence: dict[str, Any] = {}
        connection.send({"ready": True, "providers": active})
        while True:
            request = connection.recv()
            if request is None:
                return
            try:
                batch = preprocess(request, settings)
                output = session.run(None, {inputs[0].name: batch})[0]
                if not isinstance(output, np.ndarray):
                    raise InputError("model_contract", "OCR output must be a dense tensor.")
                text, score = decode(output, characters)
                if not execution_evidence:
                    profile_path = Path(session.end_profiling())
                    events = json.loads(profile_path.read_text(encoding="utf-8"))
                    nodes = [
                        e
                        for e in events
                        if e.get("cat") == "Node" and e.get("args", {}).get("provider")
                    ]
                    counts = {}
                    for event in nodes:
                        provider = event["args"]["provider"]
                        counts[provider] = counts.get(provider, 0) + 1
                    expensive_cpu = [
                        e
                        for e in nodes
                        if e["args"].get("provider") == "CPUExecutionProvider"
                        and e["args"].get("op_name") in {"Conv", "MatMul", "Gemm", "Attention"}
                    ]
                    if settings.device == "cuda" and (
                        not counts.get("CUDAExecutionProvider") or expensive_cpu
                    ):
                        raise InputError(
                            "gpu_unavailable",
                            "GPU profiling did not prove CUDA execution of the "
                            "expensive model operators.",
                        )
                    execution_evidence = {
                        "active_providers": active,
                        "profile_node_counts": counts,
                        "cpu_preprocessing": True,
                        "preprocessing": "bgr48-pillow-bilinear-ctc-v1",
                        "character_count": len(characters),
                        "input_shape": list(batch.shape),
                    }
                    profile_path.unlink(missing_ok=True)
                    profile_path = None
                connection.send(
                    {
                        "text": text,
                        "confidence": score,
                        "device": settings.device,
                        "evidence": execution_evidence,
                    }
                )
            except InputError as exc:
                connection.send({"error": exc.code, "message": str(exc)})
            except Exception:
                connection.send(
                    {
                        "error": "inference_failed",
                        "message": "Local model inference failed; request content was not logged.",
                    }
                )
    except InputError as exc:
        connection.send({"error": exc.code, "message": str(exc)})
    except Exception:
        try:
            connection.send(
                {
                    "error": "gpu_unavailable"
                    if settings.device == "cuda"
                    else "runtime_unavailable",
                    "message": "Inference runtime initialization failed; check runtime DLLs and "
                    "model contract.",
                }
            )
        except (BrokenPipeError, EOFError, OSError):
            pass
    finally:
        session = None
        if profile_path is not None:
            profile_path.unlink(missing_ok=True)
        if dll_handle is not None:
            dll_handle.close()
        connection.close()
