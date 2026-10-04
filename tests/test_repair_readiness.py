import asyncio
import threading

import httpx

from numera_cha_solver.api import create_app
from numera_cha_solver.config import Settings
from numera_cha_solver.models import ModelRegistry


def test_readiness_io_does_not_block_arithmetic_or_multiply_refreshes(tmp_path, monkeypatch):
    entered, release = threading.Event(), threading.Event()
    calls = []

    def verify(self, model):
        calls.append(model)
        entered.set()
        assert release.wait(3)
        return {"availability": "model_missing"}

    monkeypatch.setattr(ModelRegistry, "verify", verify)

    async def run():
        app = create_app(Settings(root=tmp_path, api_token="test-only"))
        async with app.router.lifespan_context(app):
            async with httpx.AsyncClient(
                transport=httpx.ASGITransport(app=app), base_url="http://localhost"
            ) as client:
                health = asyncio.create_task(client.get("/health/ready"))
                try:
                    assert await asyncio.to_thread(entered.wait, 1)
                    response = await asyncio.wait_for(
                        client.post(
                            "/v1/recognize",
                            headers={"Authorization": "Bearer test-only"},
                            json={"kind": "text", "text": "1+2"},
                        ),
                        1,
                    )
                    assert response.json()["answer"] == "3"
                    probes = [asyncio.create_task(client.get("/health/ready")) for _ in range(3)]
                    release.set()
                    await asyncio.gather(health, *probes)
                    assert len(calls) == 1
                finally:
                    release.set()
                    await asyncio.gather(health, return_exceptions=True)

    asyncio.run(run())
