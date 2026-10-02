import asyncio

import pytest

from numera_cha_solver.sdk import Numera


def test_arithmetic_recognition_does_not_claim_server_acceptance():
    result = asyncio.run(Numera().recognize({"kind": "text", "text": "12 * 3 = ?"}))
    assert result.outcome == "recognized"
    assert result.answer == "36"
    assert result.server_verified is False


@pytest.mark.parametrize("text", ["2 ** 100", "__import__('os')", "9 / 0", "99999999 + 1", ""])
def test_unsupported_arithmetic_is_rejected(text):
    result = asyncio.run(Numera().recognize({"kind": "text", "text": text}))
    assert result.outcome == "failed"
    assert result.error.code == "invalid_expression"


def test_sdk_exposes_the_shared_local_interfaces(tmp_path):
    from numera_cha_solver.config import Settings

    async def run():
        sdk = Numera(Settings(root=tmp_path))
        try:
            observation = {"kind": "text", "text": "7+5"}
            assert (await sdk.detect(observation))["provider"] == "unknown"
            assert (await sdk.solve(observation)).answer == "12"
            assert (
                len({row["catalog_id"] for row in (await sdk.capabilities())["capabilities"]}) == 26
            )
            assert (await sdk.models())[0]["availability"] == "model_missing"
            assert (await sdk.doctor())["gpu_execution_verified"] is False
            unsupported = await sdk.recognize({**observation, "subtype": "audio_digits"})
            assert unsupported.outcome == "unsupported"
            assert unsupported.answer is None
        finally:
            sdk.close()

    asyncio.run(run())


@pytest.mark.parametrize(
    "value",
    [
        {"kind": "text", "text": "1+2", "endpoint": "https://example.com"},
        {"kind": "text", "text": 3},
        {"kind": "image", "asset": "https://example.com/image"},
        {"kind": "unknown"},
        {"kind": []},
        None,
        [],
    ],
)
def test_sdk_invalid_inputs_are_structured_failures(value):
    sdk = Numera()
    try:
        result = asyncio.run(sdk.recognize(value))
        assert result.outcome == "failed"
        assert result.error.code == "invalid_input"
    finally:
        sdk.close()


def test_sdk_expired_and_unsupported_requests_do_not_recognize(tmp_path):
    from numera_cha_solver.config import Settings

    async def run():
        sdk = Numera(Settings(root=tmp_path))
        try:
            assert (
                await sdk.recognize({"kind": "text", "text": "1+2"}, deadline=0)
            ).outcome == "expired"
            assert (
                await sdk.recognize({"kind": "audio", "asset": b"owned"})
            ).outcome == "unsupported"
        finally:
            sdk.close()

    asyncio.run(run())


def test_sync_sdk_rejects_running_loop():
    async def run():
        with pytest.raises(RuntimeError, match="await"):
            Numera().recognize_sync({"kind": "text", "text": "1+2"})

    asyncio.run(run())
