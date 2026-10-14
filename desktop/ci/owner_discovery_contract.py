"""CI-only identity discovery regressions; no application or external process starts."""

import importlib.util
import json
import os
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

import psutil


def verify(work):
    spec = importlib.util.spec_from_file_location("numera_owned_contract", Path(__file__).with_name("run_owned.py"))
    owner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(owner)
    record = work / "discovery.json"
    parent = SimpleNamespace(pid=os.getpid())
    known = mock.Mock(pid=os.getpid() + 100000)
    known.create_time.return_value = 100.0
    known.is_running.return_value = True
    known.uids.return_value = SimpleNamespace(real=os.getuid())
    known.children.return_value = []
    known.environ.side_effect = psutil.AccessDenied(known.pid)
    tree = owner.OwnedTree.__new__(owner.OwnedTree)
    tree.parent = parent
    tree.parent_created = 99.0
    tree.members = {known.pid: (known, 100.0)}
    tree.process = SimpleNamespace(pid=known.pid)
    tree.tag = "owned-discovery-fixture"
    tree.record = record
    adopted = mock.Mock()
    adopted.children.return_value = []

    def snapshot(processes):
        with mock.patch.object(owner.psutil, "process_iter", return_value=processes), mock.patch.object(owner.psutil, "Process", return_value=adopted):
            tree.snapshot()

    try:
        snapshot([known])
        known.environ.assert_not_called()
        assert json.loads(record.read_text(encoding="utf-8"))["root_pid"] == known.pid
        ended = mock.Mock(pid=known.pid + 1)
        ended.is_running.return_value = True
        ended.create_time.return_value = 101.0
        ended.uids.return_value = SimpleNamespace(real=os.getuid())
        ended.status.return_value = psutil.STATUS_ZOMBIE
        snapshot([ended])
        ended.environ.assert_not_called()
        assert ended.pid not in tree.members
        ended.status.side_effect = [psutil.STATUS_RUNNING, psutil.STATUS_ZOMBIE]
        ended.environ.side_effect = psutil.AccessDenied(ended.pid)
        snapshot([ended])
        ended.environ.assert_called_once()
        assert ended.pid not in tree.members
        denied = mock.Mock(pid=known.pid + 2)
        denied.is_running.return_value = True
        denied.create_time.return_value = 101.0
        denied.uids.return_value = SimpleNamespace(real=os.getuid())
        denied.status.return_value = psutil.STATUS_RUNNING
        denied.environ.side_effect = psutil.AccessDenied(denied.pid)
        try:
            snapshot([denied])
        except RuntimeError as error:
            assert "new live same-user process" in str(error)
        else:
            raise AssertionError("Live unknown inaccessible process must fail closed.")
        assert denied.pid not in tree.members
    finally:
        record.unlink(missing_ok=True)
