import asyncio
import json

import pytest

from numera_cha_solver.benchmark import benchmark
from numera_cha_solver.config import Settings
from numera_cha_solver.core.contracts import InputError
from numera_cha_solver.sdk import Numera


def test_duplicate_asset_cannot_promote_independent_evidence(tmp_path):
    (tmp_path / "one.png").write_bytes(b"owned synthetic bytes")
    manifest = tmp_path / "samples.json"
    manifest.write_text(
        json.dumps(
            {
                "license": "owned",
                "kind": "held_out",
                "samples": [
                    {"label": "x", "session_id": "a", "path": "one.png"},
                    {"label": "x", "session_id": "b", "path": "./one.png"},
                ],
            }
        ),
        encoding="utf-8",
    )
    sdk = Numera(Settings(root=tmp_path))
    try:
        with pytest.raises(InputError, match="Duplicate"):
            asyncio.run(benchmark(sdk, manifest))
    finally:
        sdk.close()


def test_renamed_duplicate_content_is_rejected(tmp_path):
    (tmp_path / "one.png").write_bytes(b"owned bytes")
    (tmp_path / "two.png").write_bytes(b"owned bytes")
    manifest = tmp_path / "samples.json"
    manifest.write_text(
        json.dumps(
            {
                "license": "owned",
                "samples": [
                    {"label": "x", "session_id": "a", "path": "one.png"},
                    {"label": "x", "session_id": "b", "path": "two.png"},
                ],
            }
        ),
        encoding="utf-8",
    )
    sdk = Numera(Settings(root=tmp_path))
    try:
        with pytest.raises(InputError, match="Duplicate asset content"):
            asyncio.run(benchmark(sdk, manifest))
    finally:
        sdk.close()
