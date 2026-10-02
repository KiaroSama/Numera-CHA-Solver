import pytest

from numera_cha_solver.ownership import ServerOwnership


def test_second_server_cannot_take_active_store_and_lock_releases(tmp_path):
    path = tmp_path / "ownership.lock"
    first, second = ServerOwnership(path), ServerOwnership(path)
    first.acquire()
    try:
        with pytest.raises(RuntimeError, match="Another server"):
            second.acquire()
    finally:
        first.close()
    second.acquire()
    second.close()
