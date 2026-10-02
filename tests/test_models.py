import pytest

from numera_cha_solver.config import Settings
from numera_cha_solver.core.contracts import InputError
from numera_cha_solver.models import ModelRegistry


def test_missing_models_report_unavailable_without_download(tmp_path):
    registry = ModelRegistry(Settings(root=tmp_path))
    status = registry.verify("ppocr-v6-small")
    assert status["availability"] == "model_missing"
    assert not (tmp_path / "data" / "models").exists()


def test_corrupt_model_assets_are_not_ready(tmp_path):
    registry = ModelRegistry(Settings(root=tmp_path))
    directory = registry.directory("ppocr-v6-small")
    directory.mkdir(parents=True)
    (directory / "inference.onnx").write_bytes(b"not reviewed weights")
    status = registry.verify("ppocr-v6-small")
    assert status["availability"] == "model_corrupt"
    assert {row["reason"] for row in status["failures"]} == {"missing", "size_or_path_mismatch"}
    with pytest.raises(InputError) as failure:
        registry.get("unknown")
    assert failure.value.code == "unknown_model"
