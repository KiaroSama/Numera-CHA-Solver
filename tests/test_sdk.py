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


def test_sync_sdk_rejects_running_loop():
    async def run():
        with pytest.raises(RuntimeError, match="await"):
            Numera().recognize_sync({"kind": "text", "text": "1+2"})

    asyncio.run(run())
