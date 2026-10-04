import contextlib
import io
import sys

from fastapi.testclient import TestClient

from numera_cha_solver.api import create_app
from numera_cha_solver.cli import main
from numera_cha_solver.config import Settings


def test_competing_cli_start_preserves_active_credential(tmp_path, monkeypatch):
    settings = Settings(root=tmp_path, api_token="owned-test-credential")
    monkeypatch.setenv("NUMERA_ROOT", str(tmp_path))
    monkeypatch.delenv("NUMERA_API_TOKEN", raising=False)
    monkeypatch.setattr(sys, "argv", ["numera-cha", "serve"])

    def start(app, **kwargs):
        with TestClient(app):
            pass

    monkeypatch.setattr("uvicorn.run", start)
    with TestClient(create_app(settings)) as client:
        credential = settings.data_dir / "api-token.txt"
        credential.write_text(settings.api_token, encoding="utf-8")
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            result = main()
        assert credential.read_text(encoding="utf-8") == settings.api_token
        assert (
            client.get(
                "/v1/models", headers={"Authorization": "Bearer owned-test-credential"}
            ).status_code
            == 200
        )
        assert result == 2


def test_occupied_port_leaves_previous_credential_untouched(tmp_path, monkeypatch):
    import socket

    monkeypatch.setenv("NUMERA_ROOT", str(tmp_path))
    monkeypatch.delenv("NUMERA_API_TOKEN", raising=False)
    (tmp_path / "data").mkdir()
    path = tmp_path / "data/api-token.txt"
    path.write_text("owned-old-value", encoding="utf-8")
    with socket.socket() as occupied:
        occupied.bind(("127.0.0.1", 0))
        occupied.listen()
        monkeypatch.setattr(
            sys, "argv", ["numera-cha", "serve", "--port", str(occupied.getsockname()[1])]
        )
        with contextlib.redirect_stdout(io.StringIO()):
            assert main() == 2
    assert path.read_text(encoding="utf-8") == "owned-old-value"


def test_lifespan_failure_does_not_publish_new_credential(tmp_path, monkeypatch):
    import socket

    import pytest
    import uvicorn

    from numera_cha_solver.startup import serve

    settings = Settings(root=tmp_path)
    settings.data_dir.mkdir()
    (settings.data_dir / "tasks.sqlite3").mkdir()
    credential = settings.data_dir / "api-token.txt"
    credential.write_text("owned-old-value", encoding="utf-8")

    def in_process(server, sockets=None):
        with TestClient(server.config.app):
            pass

    monkeypatch.setattr(uvicorn.Server, "run", in_process)
    with socket.socket() as candidate:
        candidate.bind(("127.0.0.1", 0))
        port = candidate.getsockname()[1]
    with pytest.raises(Exception, match="database"):
        serve(settings, "127.0.0.1", port)
    assert credential.read_text(encoding="utf-8") == "owned-old-value"
