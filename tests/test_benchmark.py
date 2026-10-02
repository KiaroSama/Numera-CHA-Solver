import asyncio
import json

import pytest

from numera_cha_solver.benchmark import benchmark
from numera_cha_solver.config import Settings
from numera_cha_solver.core.contracts import InputError
from numera_cha_solver.sdk import Numera


def test_benchmark_rejects_scalar_manifest_without_crashing(tmp_path):
    manifest = tmp_path / "samples.json"
    manifest.write_text("null", encoding="utf-8")
    sdk = Numera(Settings(root=tmp_path))
    try:
        with pytest.raises(InputError, match="manifest"):
            asyncio.run(benchmark(sdk, manifest))
    finally:
        sdk.close()


@pytest.mark.parametrize(
    "document",
    [
        [],
        {"license": "owned", "samples": [None]},
        {"license": [], "samples": [{}]},
        {"license": "owned", "samples": [{"label": "x", "session_id": [], "path": "x.png"}]},
        {"license": "owned", "samples": [{"label": "x", "session_id": "a", "path": "../x.png"}]},
        {
            "license": "owned",
            "samples": [
                {"label": "x", "session_id": "a", "path": "x.png"},
                {"label": "y", "session_id": "a", "path": "y.png"},
            ],
        },
    ],
)
def test_invalid_benchmark_samples_fail_before_inference(tmp_path, document):
    manifest = tmp_path / "samples.json"
    manifest.write_text(json.dumps(document), encoding="utf-8")
    sdk = Numera(Settings(root=tmp_path))
    try:
        with pytest.raises(InputError) as failure:
            asyncio.run(benchmark(sdk, manifest))
        assert failure.value.code == "invalid_benchmark"
        assert not (tmp_path / "data").exists()
    finally:
        sdk.close()


@pytest.mark.parametrize(
    "content",
    [b"{", b"\xff", b" " * (1024 * 1024 + 1)],
    ids=["malformed-json", "invalid-utf8", "oversized"],
)
def test_benchmark_decode_errors_are_sanitized(tmp_path, content):
    manifest = tmp_path / "samples.json"
    manifest.write_bytes(content)
    sdk = Numera(Settings(root=tmp_path))
    try:
        with pytest.raises(InputError) as failure:
            asyncio.run(benchmark(sdk, manifest))
        assert failure.value.code == "invalid_benchmark"
    finally:
        sdk.close()


def test_benchmark_keeps_unreadable_and_failed_samples_in_denominator(tmp_path):
    (tmp_path / "invalid.png").write_bytes(b"not an image")
    manifest = tmp_path / "samples.json"
    manifest.write_text(
        json.dumps(
            {
                "license": "owned test fixture",
                "kind": "synthetic",
                "samples": [
                    {"label": "x", "session_id": "one", "path": "missing.png"},
                    {"label": "y", "session_id": "two", "path": "invalid.png"},
                ],
            }
        ),
        encoding="utf-8",
    )
    sdk = Numera(Settings(root=tmp_path))
    try:
        result = asyncio.run(benchmark(sdk, manifest))
        assert result["sample_count"] == 2
        assert result["correct"] == 0
        assert result["exact_accuracy"] == 0
        assert [row["outcome"] for row in result["rows"]] == ["failed", "failed"]
        assert result["limited_evidence"] is True
        assert result["live_acceptance_measured"] is False
        assert result["confidence_interval_95"][0] == 0
    finally:
        sdk.close()
