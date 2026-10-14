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
    tree.inspection_failures = []
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
        ended.parent.return_value = None
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
        denied.parent.return_value = None
        denied.ppid.return_value = 1
        try:
            snapshot([denied])
        except RuntimeError as error:
            assert "new live same-user process" in str(error)
        else:
            raise AssertionError("Live unknown inaccessible process must fail closed.")
        assert denied.pid not in tree.members
        late = mock.Mock(pid=known.pid + 3)
        late.is_running.return_value = True
        late.create_time.return_value = 102.0
        late.uids.return_value = SimpleNamespace(real=os.getuid())
        late.status.return_value = psutil.STATUS_RUNNING
        late.children.return_value = []
        late.parent.return_value = known
        late.environ.side_effect = psutil.AccessDenied(late.pid)
        snapshot([late])
        late.environ.assert_not_called()
        assert tree.members[late.pid][1] == 102.0
        tree.members.pop(late.pid)
        late.parent.side_effect = [None, known]
        snapshot([late])
        late.environ.assert_called_once()
        assert late.pid in tree.members
        tree.members.pop(late.pid)
        reused_parent = mock.Mock(pid=known.pid)
        reused_parent.is_running.return_value = True
        reused_parent.create_time.return_value = 103.0
        reused_parent.parent.return_value = None
        late.parent.side_effect = None
        late.parent.return_value = reused_parent
        assert not tree.attribute(late), "Reused parent PID cannot confer ownership."
        assert late.pid not in tree.members
        reused_parent.parent.side_effect = psutil.NoSuchProcess(reused_parent.pid)
        late.environ.reset_mock()
        late.environ.side_effect = None
        late.environ.return_value = {owner.OWNER_TAG: tree.tag}
        snapshot([late])
        late.environ.assert_called_once()
        assert late.pid in tree.members, "A vanished ancestor must not hide a live tagged candidate."
        tree.members.pop(late.pid)
        known.status.return_value = psutil.STATUS_ZOMBIE
        assert not tree.executing(), "An ended identity is not an executing leak."
        tree.process = mock.Mock(pid=known.pid + 20)
        with mock.patch.object(owner.os, "waitpid", return_value=(known.pid, 0)) as wait:
            tree.reap_ended()
            wait.assert_called_once_with(known.pid, os.WNOHANG)
            tree.process.poll.assert_called_once()
        inaccessible = mock.Mock(pid=known.pid + 30)
        inaccessible.is_running.return_value = True
        inaccessible.create_time.return_value = 104.0
        inaccessible.status.side_effect = psutil.AccessDenied(inaccessible.pid)
        known.status.return_value = psutil.STATUS_RUNNING
        tree.members[inaccessible.pid] = (inaccessible, 104.0)
        with mock.patch.object(tree, "snapshot"), mock.patch.object(tree, "reap_ended"), mock.patch.object(tree, "live", side_effect=[list([known, inaccessible]), list([known, inaccessible]), []]):
            try:
                tree.stop()
            except RuntimeError as error:
                assert "known children reaped" in str(error)
            else:
                raise AssertionError("Unavailable owned status must remain a cleanup failure.")
        known.kill.assert_called_once()
        inaccessible.kill.assert_called_once()
    finally:
        record.unlink(missing_ok=True)
