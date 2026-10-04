import contextlib
import io
import json
import sys

import pytest

from numera_cha_solver.cli import main


@pytest.mark.parametrize("content", ["{", "null", '{"dll_directory": 7}'])
def test_saved_config_failure_is_one_sanitized_cli_result(tmp_path, monkeypatch, content):
    data = tmp_path / "data"
    data.mkdir()
    (data / "runtime-config.json").write_text(content, encoding="utf-8")
    monkeypatch.setenv("NUMERA_ROOT", str(tmp_path))
    monkeypatch.delenv("NUMERA_DLL_DIRECTORY", raising=False)
    monkeypatch.setattr(sys, "argv", ["numera-cha", "doctor"])
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        code = main()
    assert code == 2
    assert json.loads(output.getvalue())["error"]["code"] == "invalid_config"
    assert list((tmp_path / "logs").glob("*.log"))
