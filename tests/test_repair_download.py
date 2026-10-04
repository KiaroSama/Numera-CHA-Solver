import pytest

from numera_cha_solver.config import Settings
from numera_cha_solver.core.contracts import InputError
from numera_cha_solver.models import ModelRegistry


def test_stale_download_file_does_not_establish_active_owner(tmp_path, monkeypatch):
    registry = ModelRegistry(Settings(root=tmp_path))
    directory = registry.directory("ppocr-v6-small")
    directory.mkdir(parents=True)
    (directory / ".download.lock").write_bytes(b"stale")

    def offline(*args):
        raise InputError("offline_boundary", "Reached controlled download boundary.")

    monkeypatch.setattr(ModelRegistry, "_download", offline)
    with pytest.raises(InputError) as failure:
        registry.fetch("ppocr-v6-small")
    assert failure.value.code == "offline_boundary"


def hold_download(root, entered, release):
    registry = ModelRegistry(Settings(root=root))

    def pause(*args):
        entered.set()
        release.wait(10)
        raise InputError("offline_boundary", "Controlled offline boundary.")

    registry._download = pause
    try:
        registry.fetch("ppocr-v6-small")
    except InputError:
        pass


def test_download_crash_releases_lease_and_live_contender_stays_busy(tmp_path, monkeypatch):
    import multiprocessing as mp

    context = mp.get_context("spawn")
    entered, release = context.Event(), context.Event()
    process = context.Process(target=hold_download, args=(tmp_path, entered, release))
    process.start()
    try:
        assert entered.wait(5)
        registry = ModelRegistry(Settings(root=tmp_path))
        with pytest.raises(InputError) as contender:
            registry.fetch("ppocr-v6-small")
        assert contender.value.code == "model_busy"
        process.terminate()
        process.join(5)
        assert not process.is_alive()

        def offline(*args):
            raise InputError("offline_boundary", "Reached recovered download boundary.")

        monkeypatch.setattr(ModelRegistry, "_download", offline)
        with pytest.raises(InputError) as recovered:
            registry.fetch("ppocr-v6-small")
        assert recovered.value.code == "offline_boundary"
    finally:
        release.set()
        if process.is_alive():
            process.terminate()
        process.join(5)
        process.close()
