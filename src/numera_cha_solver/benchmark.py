"""Small evaluation reports preserve failures and abstentions in denominators."""

import asyncio
import hashlib
import json
import math
import statistics
import time
from pathlib import Path
from typing import Any

from .assets import read_asset
from .core.contracts import Error, InputError, Result
from .sdk import Numera


def wilson(correct: int, count: int) -> list[float] | None:
    if count == 0:
        return None
    z = 1.959963984540054
    p = correct / count
    center = (p + z * z / (2 * count)) / (1 + z * z / count)
    margin = z * math.sqrt(p * (1 - p) / count + z * z / (4 * count * count)) / (1 + z * z / count)
    return [max(0, center - margin), min(1, center + margin)]


def read_manifest(manifest: Path) -> tuple[dict[str, Any], list[tuple[dict[str, Any], Path]]]:
    try:
        with manifest.open("rb") as stream:
            content = stream.read(1024 * 1024 + 1)
    except OSError:
        raise InputError("invalid_benchmark", "Benchmark manifest could not be read.") from None
    if len(content) > 1024 * 1024:
        raise InputError("invalid_benchmark", "Benchmark manifest exceeds its size limit.")
    try:
        document = json.loads(content.decode("utf-8"))
    except (ValueError, RecursionError):
        raise InputError(
            "invalid_benchmark", "Benchmark manifest must be valid UTF-8 JSON."
        ) from None
    if not isinstance(document, dict):
        raise InputError("invalid_benchmark", "Benchmark manifest must be a JSON object.")
    samples = document.get("samples")
    license_name = document.get("license")
    if (
        not isinstance(samples, list)
        or not 1 <= len(samples) <= 1000
        or not isinstance(license_name, str)
        or not license_name.strip()
        or len(license_name) > 256
        or not isinstance(document.get("kind", "unverified"), str)
    ):
        raise InputError("invalid_benchmark", "Use 1-1000 labeled licensed samples.")
    identities: set[str] = set()
    paths: set[Path] = set()
    validated = []
    root = manifest.parent.resolve()
    for sample in samples:
        if not isinstance(sample, dict):
            raise InputError("invalid_benchmark", "Each benchmark sample must be an object.")
        label, identity, name = sample.get("label"), sample.get("session_id"), sample.get("path")
        if (
            not isinstance(label, str)
            or len(label) > 4096
            or not isinstance(identity, str)
            or not identity.strip()
            or len(identity) > 128
            or identity in identities
            or not isinstance(name, str)
            or not name
            or len(name) > 4096
        ):
            raise InputError(
                "invalid_benchmark", "Each sample needs a unique identity, label and path."
            )
        source_id = sample.get("source_id", identity)
        if not isinstance(source_id, str) or not source_id.strip() or len(source_id) > 128:
            raise InputError("invalid_benchmark", "Source identity must be a bounded string.")
        identities.add(identity)
        try:
            relative = Path(name)
            path = (root / relative).resolve()
        except (OSError, ValueError):
            raise InputError("invalid_benchmark", "Sample path is invalid.") from None
        if relative.is_absolute() or not path.is_relative_to(root):
            raise InputError(
                "invalid_benchmark", "Sample paths must remain inside the manifest directory."
            )
        if path in paths:
            raise InputError(
                "invalid_benchmark",
                "Duplicate canonical sample paths are not independent evidence.",
            )
        paths.add(path)
        validated.append((sample, path))
    return document, validated


async def benchmark(sdk: Numera, manifest: Path) -> dict[str, Any]:
    document, samples = await asyncio.to_thread(read_manifest, manifest)
    hashes: dict[Path, str] = {}
    seen_hashes: set[str] = set()
    for _, path in samples:
        try:
            content = await asyncio.to_thread(read_asset, path, sdk.settings)
        except (InputError, OSError):
            continue
        digest = hashlib.sha256(content).hexdigest()
        if digest in seen_hashes:
            raise InputError(
                "invalid_benchmark", "Duplicate asset content is not independent evidence."
            )
        seen_hashes.add(digest)
        hashes[path] = digest
    rows: list[dict[str, Any]] = []
    for sample, path in samples:
        started = time.monotonic()
        try:
            content = await asyncio.to_thread(read_asset, path, sdk.settings)
            if hashlib.sha256(content).hexdigest() != hashes.get(path):
                raise InputError("invalid_asset", "Sample changed after manifest validation.")
            result = await sdk.recognize({"kind": "image", "asset": content})
        except (InputError, OSError):
            result = Result(
                "failed", error=Error("invalid_asset", "Benchmark sample could not be safely read.")
            )
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
        "unique_asset_count": len(seen_hashes),
        "independent_unit_count": len(
            {sample.get("source_id", sample["session_id"]) for sample, _ in samples}
        ),
        "split_provenance": document.get("split_provenance", "unverified"),
        "limited_evidence": True,
        "qualification": (
            "Exact duplicates excluded; near-duplicates and training/calibration overlap "
            "remain unverified. Promotion gates have not been evaluated."
        ),
        "live_acceptance_measured": False,
        "rows": rows,
    }
