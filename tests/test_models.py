from numera_cha_solver.config import Settings
from numera_cha_solver.models import ModelRegistry


def test_missing_models_report_unavailable_without_download(tmp_path):
    registry = ModelRegistry(Settings(root=tmp_path))
    status = registry.verify("ppocr-v6-small")
    assert status["availability"] == "model_missing"
    assert not (tmp_path / "data" / "models").exists()
