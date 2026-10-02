import asyncio
import os
from pathlib import Path

import pytest

from numera_cha_solver.assets import read_asset
from numera_cha_solver.config import Settings
from numera_cha_solver.sdk import Numera


@pytest.mark.gpu
@pytest.mark.timeout(60)
def test_owned_ascii_real_model_gpu_execution():
    if os.environ.get("NUMERA_TEST_GPU") != "1":
        pytest.skip("Real GPU tier was not selected; no GPU evidence is claimed.")
    root = Path(os.environ.get("NUMERA_RUNTIME_ROOT", Path(__file__).resolve().parents[1]))
    settings = Settings.from_env(root=root)
    sdk = Numera(settings)
    try:
        asset = read_asset(Path(__file__).parent / "fixtures/ocr/ascii-smoke.png", settings)
        result = asyncio.run(sdk.recognize({"kind": "image", "asset": asset}))
        assert result.answer == "NUMERA 123"
        assert result.device == "cuda"
        assert result.evidence["profile_node_counts"]["CUDAExecutionProvider"] > 0
        assert result.server_verified is False
    finally:
        sdk.close()
