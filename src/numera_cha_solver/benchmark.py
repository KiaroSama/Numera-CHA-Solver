"""Small evaluation reports preserve failures and abstentions in denominators."""

import asyncio
import json
import math
import statistics
import time
from pathlib import Path
from typing import Any

from .assets import read_asset
from .core.contracts import InputError
from .sdk import Numera


def wilson(correct: int, count: int) -> list[float] | None:
    if count == 0:
        return None
    z = 1.959963984540054
    p = correct / count
    center = (p + z * z / (2 * count)) / (1 + z * z / count)
    margin = z * math.sqrt(p * (1 - p) / count + z * z / (4 * count * count)) / (1 + z * z / count)
    return [max(0, center - margin), min(1, center + margin)]


async def benchmark(sdk: Numera, manifest: Path) -> dict[str, Any]:
    if (await asyncio.to_thread(manifest.stat)).st_size > 1024 * 1024:
        raise InputError("invalid_benchmark", "Benchmark manifest exceeds its size limit.")
    document = json.loads(await asyncio.to_thread(manifest.read_text, encoding="utf-8"))
    samples = document.get("samples")
    if (
        not isinstance(samples, list)
        or not 1 <= len(samples) <= 1000
        or not document.get("license")
    ):
        raise InputError("invalid_benchmark", "Use 1-1000 labeled licensed samples.")
    rows: list[dict[str, Any]] = []
    identities = set()
    for sample in samples:
        if (
            not isinstance(sample.get("label"), str)
            or not sample.get("session_id")
            or sample["session_id"] in identities
        ):
            raise InputError(
                "invalid_benchmark",
                "Each sample needs a distinct challenge/session identity and text label.",
            )
        identities.add(sample["session_id"])
        path = (manifest.parent / sample["path"]).resolve()
        if not path.is_relative_to(manifest.parent.resolve()):
            raise InputError(
                "invalid_benchmark", "Sample paths must remain inside the manifest directory."
            )
        started = time.monotonic()
        result = await sdk.recognize({"kind": "image", "asset": read_asset(path, sdk.settings)})
        rows.append(
            {
                "session_id": sample["session_id"],
                "correct": result.outcome == "recognized" and result.answer == sample["label"],
                "outcome": result.outcome,
                "abstained": result.abstained,
                "duration_ms": round((time.monotonic() - started) * 1000, 2),
                "device": result.device,
                "model_revision": result.model_revision,
            }
        )
    count = len(rows)
    correct = sum(row["correct"] for row in rows)
    latencies = sorted(row["duration_ms"] for row in rows)
    return {
        "sample_count": count,
        "correct": correct,
        "exact_accuracy": correct / count,
        "abstention_rate": sum(row["abstained"] for row in rows) / count,
        "confidence_interval_95": wilson(correct, count),
        "p50_ms": statistics.median(latencies),
        "p95_ms": latencies[min(count - 1, math.ceil(0.95 * count) - 1)],
        "license": document["license"],
        "dataset_kind": document.get("kind", "unverified"),
        "limited_evidence": count < 100 or document.get("kind") != "held_out",
        "live_acceptance_measured": False,
        "rows": rows,
    }
