import sqlite3

import pytest
from fastapi.testclient import TestClient

from numera_cha_solver.api import create_app
from numera_cha_solver.config import Settings


def test_startup_failure_releases_owned_store_for_retry(tmp_path):
    settings = Settings(root=tmp_path, api_token="test-only")
    settings.data_dir.mkdir()
    database = settings.data_dir / "tasks.sqlite3"
    database.mkdir()
    app = create_app(settings)
    with pytest.raises(sqlite3.OperationalError, match="database"):
        with TestClient(app):
            pass
    database.rmdir()
    with TestClient(app) as client:
        assert client.get("/health/live").status_code == 200


def test_api_session_input_and_host_boundaries(tmp_path):
    with TestClient(create_app(Settings(root=tmp_path, api_token="test-only"))) as client:
        assert (
            client.post("/v1/session", headers={"Authorization": "Bearer test-only"}).status_code
            == 200
        )
        assert client.get("/v1/models").status_code == 200
        invalid = client.post("/v1/recognize", json={"kind": "image", "asset_base64": "!"})
        assert invalid.status_code == 422
        assert "request_id" in invalid.json()
        assert client.get("/health/live", headers={"Host": "example.com"}).status_code == 403
        result = client.post(
            "/v1/recognize", json={"kind": "text", "text": "1+2", "subtype": "grid"}
        )
        assert result.json()["outcome"] == "unsupported"
        assert client.post("/v1/tasks", json={"kind": "text", "text": "1+2"}).status_code == 400
        assert client.get("/v1/tasks/missing").status_code == 404


def test_api_shares_sdk_and_requires_local_client_authorization(tmp_path):
    with TestClient(create_app(Settings(root=tmp_path, api_token="test-only"))) as client:
        assert client.get("/health/live").status_code == 200
        assert client.get("/v1/capabilities").status_code == 401
        headers = {"Authorization": "Bearer test-only"}
        result = client.post(
            "/v1/recognize", headers=headers, json={"kind": "text", "text": "12*3"}
        )
        assert result.status_code == 200
        assert result.json()["answer"] == "36"
        assert result.json()["server_verified"] is False
        assert (
            client.get(
                "/v1/models", headers={**headers, "Origin": "https://example.com"}
            ).status_code
            == 403
        )
        created = client.post(
            "/v1/tasks",
            headers={**headers, "Idempotency-Key": "same"},
            json={"kind": "text", "text": "12*3"},
        )
        duplicate = client.post(
            "/v1/tasks",
            headers={**headers, "Idempotency-Key": "same"},
            json={"kind": "text", "text": "12*3"},
        )
        assert created.json()["id"] == duplicate.json()["id"]
        cancelled = client.post(f"/v1/tasks/{created.json()['id']}/cancel", headers=headers)
        assert cancelled.json()["state"] == "terminal"
        assert (
            client.post(
                "/v1/recognize",
                headers=headers,
                json={"kind": "text", "text": "1+2", "endpoint": "https://example.com"},
            ).status_code
            == 422
        )
