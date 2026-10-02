from fastapi.testclient import TestClient

from numera_cha_solver.api import create_app
from numera_cha_solver.config import Settings


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
