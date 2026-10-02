import contextlib
import io
import json
import sys

import pytest

from numera_cha_solver.cli import main


@pytest.mark.parametrize("expression,answer,code", [("7+5", "12", 0), ("7/0", None, 4)])
def test_cli_json_and_error_exit(monkeypatch, tmp_path, expression, answer, code):
    monkeypatch.setenv("NUMERA_ROOT", str(tmp_path))
    monkeypatch.setattr(sys, "argv", ["numera-cha", "recognize", "--text", expression])
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        assert main() == code
    result = json.loads(output.getvalue())
    assert result["answer"] == answer
    assert result["server_verified"] is False
    assert len(list((tmp_path / "logs").glob("*.log"))) == 1
