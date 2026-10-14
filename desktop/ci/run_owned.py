"""Hosted POSIX owner: bounded commands, PID/create-time tracking across detached sessions."""

import argparse
import ctypes
import json
import os
import re
import selectors
import signal
import subprocess
import sys
import time
import uuid
from pathlib import Path

import psutil

OWNER_TAG = "NUMERA_CI_PROCESS_OWNER"


def safe(text):
    text = re.sub(r"(?i)bearer\s+\S+", "Bearer [REDACTED]", text)
    return re.sub(r"(?i)((?:token|secret|password|authorization|api[_-]?key)\s*[:=]\s*)\S+", r"\1[REDACTED]", text)


def group_alive(pid):
    try:
        os.killpg(pid, 0)
        return True
    except ProcessLookupError:
        return False


def enable_reaping():
    if sys.platform == "linux":
        if ctypes.CDLL(None, use_errno=True).prctl(36, 1, 0, 0, 0) != 0:
            raise OSError(ctypes.get_errno(), "Owned descendant reaping could not be enabled.")


def reap_group(process):
    if group_alive(process.pid):
        os.killpg(process.pid, signal.SIGKILL)
    process.wait(timeout=5)
    deadline = time.monotonic() + 5
    while group_alive(process.pid):
        try:
            while os.waitpid(-process.pid, os.WNOHANG)[0]:
                pass
        except ChildProcessError:
            pass
        if time.monotonic() >= deadline:
            raise RuntimeError("Owned process group survived cleanup.")
        time.sleep(0.02)  # Bounded kernel reaping readiness.


class OwnedTree:
    def __init__(self, process, tag, record):
        self.process = process
        self.tag = tag
        self.record = record
        self.parent = psutil.Process()
        self.parent_created = self.parent.create_time()
        self.members = {}
        self.remember(psutil.Process(process.pid))
        self.started = self.members[process.pid][1]
        if psutil.Process(process.pid).environ().get(OWNER_TAG) != tag:
            raise RuntimeError("Owned process environment identity is unavailable.")

    def remember(self, process):
        try:
            identity = process.create_time()
            if process.pid == os.getpid():
                raise RuntimeError("Process owner cannot own itself.")
            previous = self.members.get(process.pid)
            if previous and previous[1] != identity:
                raise RuntimeError("Tracked process PID was reused before cleanup.")
            self.members[process.pid] = (process, identity)
        except psutil.NoSuchProcess:
            pass

    def live(self):
        live = []
        for process, created in self.members.values():
            try:
                if process.is_running() and process.create_time() == created:
                    live.append(process)
            except psutil.NoSuchProcess:
                continue
        return live

    def snapshot(self):
        # Preserve identities while parents exist, then recover escaped/reparented processes by a per-command tag.
        for process in list(self.live()):
            try:
                for child in process.children(recursive=True):
                    self.remember(child)
            except psutil.NoSuchProcess:
                pass
        if sys.platform == "linux":
            for child in psutil.Process().children(recursive=True):
                self.remember(child)  # Subreaper adoption survives any setsid()/double fork.
        for process in psutil.process_iter():
            try:
                if not process.is_running() or process.pid == self.parent.pid or process.create_time() < self.parent_created or process.uids().real != os.getuid():
                    continue
                known = self.members.get(process.pid)
                if known and process.create_time() == known[1]:
                    continue  # An owned identity needs no restricted environment read after exec or exit.
                if process.status() in (psutil.STATUS_ZOMBIE, psutil.STATUS_DEAD):
                    continue  # Ended unknown processes cannot spawn descendants; never signal them.
                if process.environ().get(OWNER_TAG) == self.tag:
                    self.remember(process)
            except (psutil.NoSuchProcess, psutil.ZombieProcess):
                continue
            except psutil.AccessDenied as error:
                try:
                    if not process.is_running() or process.status() in (psutil.STATUS_ZOMBIE, psutil.STATUS_DEAD):
                        continue  # Exit may race the preceding environment inspection.
                except (psutil.NoSuchProcess, psutil.ZombieProcess):
                    continue
                raise RuntimeError("Owned descendant discovery cannot inspect a new live same-user process; refusing an unverified run.") from error
        self.record.write_text(json.dumps({"root_pid": self.process.pid,
            "members": [{"pid": p.pid, "created": created} for p, created in self.members.values()]}), encoding="utf-8")

    def stop(self):
        deadline = time.monotonic() + 5
        discovery_failure = None
        try:
            self.snapshot()
        except Exception as error:
            discovery_failure = error
        # Freeze discovered parents before killing, so they cannot create an unobserved child during teardown.
        for process in reversed(self.live()):
            try:
                process.send_signal(signal.SIGSTOP)
            except psutil.NoSuchProcess:
                pass
        try:
            self.snapshot()
        except Exception as error:
            discovery_failure = error
        while True:
            for process in reversed(self.live()):
                try:
                    process.kill()  # psutil checks PID + creation time before signalling.
                except psutil.NoSuchProcess:
                    pass
            if self.process.poll() is None:
                self.process.wait(timeout=max(0.01, deadline - time.monotonic()))
            if sys.platform == "linux":
                try:
                    while os.waitpid(-1, os.WNOHANG)[0]:
                        pass
                except ChildProcessError:
                    pass
            try:
                self.snapshot()
            except Exception as error:
                discovery_failure = error
            if not self.live():
                if discovery_failure:
                    raise RuntimeError("Owned discovery was incomplete; known children reaped but run remains failed.") from discovery_failure
                return
            if time.monotonic() >= deadline:
                raise RuntimeError("Owned descendants survived verified PID/create-time cleanup.")
            time.sleep(0.02)  # Bounded cleanup-state polling, no blind startup wait.


def launch(command):
    descriptor = int(os.environ.pop("NUMERA_CI_OWNER_GATE"))
    try:
        if os.read(descriptor, 1) != b"1":
            raise RuntimeError("Owned launch gate closed before identity capture.")
    finally:
        os.close(descriptor)
    os.execvpe(command[0], command, os.environ)


def main():
    if sys.argv[1:2] == ["--launch"]:
        launch(sys.argv[3:] if sys.argv[2:3] == ["--"] else sys.argv[2:])
        return 1
    parser = argparse.ArgumentParser()
    parser.add_argument("--cwd", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--wall", type=int, required=True)
    parser.add_argument("--idle", type=int, required=True)
    parser.add_argument("--capture", type=Path)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if os.name == "nt" or os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise RuntimeError("This process owner is restricted to GitHub-hosted POSIX CI.")
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command or not 1 <= args.wall <= 1800 or not 1 <= args.idle <= args.wall:
        raise ValueError("Bounded command parameters are invalid.")
    args.work.mkdir(parents=True, exist_ok=True)
    enable_reaping()
    def interrupt(_signum, _frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, interrupt)
    signal.signal(signal.SIGHUP, interrupt)
    started = progress = time.monotonic()
    process = tree = None
    capture = args.capture.open("wb") if args.capture else None
    read_gate, write_gate = os.pipe()
    failure = None
    try:
        tag = uuid.uuid4().hex
        env = os.environ.copy()
        env[OWNER_TAG] = tag
        env["NUMERA_CI_OWNER_GATE"] = str(read_gate)
        process = subprocess.Popen([sys.executable, "-I", __file__, "--launch", "--", *command], cwd=args.cwd,
            env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            start_new_session=True, pass_fds=(read_gate,))
        os.close(read_gate)
        read_gate = None
        record = args.work / f"owner-{process.pid}.json"
        tree = OwnedTree(process, tag, record)
        tree.snapshot()
        os.write(write_gate, b"1")
        os.close(write_gate)
        write_gate = None
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ, "stdout")
            selector.register(process.stderr, selectors.EVENT_READ, "stderr")
            output_bytes = 0
            while selector.get_map() and not failure:
                tree.snapshot()
                now = time.monotonic()
                if now - started >= args.wall or now - progress >= args.idle:
                    failure = "Owned command exceeded its wall or idle deadline."
                    break
                for key, _ in selector.select(timeout=min(0.05, args.wall - (now - started), args.idle - (now - progress))):
                    chunk = os.read(key.fileobj.fileno(), 65536)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    output_bytes += len(chunk)
                    if output_bytes > 32 * 1024 * 1024:
                        failure = "Owned command output exceeded its safety ceiling."
                        break
                    progress = time.monotonic()
                    if capture and key.data == "stdout":
                        capture.write(chunk)
                    else:
                        sys.stderr.write(safe(chunk.decode("utf-8", errors="replace")))
                        sys.stderr.flush()
            if not failure:
                while process.poll() is None:
                    tree.snapshot()
                    if time.monotonic() - started >= args.wall or time.monotonic() - progress >= args.idle:
                        raise subprocess.TimeoutExpired(command, args.wall)
                    time.sleep(0.02)
                code = process.returncode
                tree.snapshot()
                if tree.live():
                    failure = "Owned command exited with surviving descendants, including detached sessions."
            else:
                code = 124
        if failure:
            sys.stderr.write(failure + "\n")
            return 124 if code == 124 else 1
        return code
    except (KeyboardInterrupt, subprocess.TimeoutExpired):
        sys.stderr.write("Owned command interrupted or exceeded its exit deadline.\n")
        return 124
    finally:
        for descriptor in (read_gate, write_gate):
            if descriptor is not None:
                os.close(descriptor)
        try:
            if tree is not None:
                tree.stop()
                record.unlink(missing_ok=True)
            elif process is not None:
                # Gate never opened, so the trampoline has no descendants.
                reap_group(process)
        finally:
            if process is not None:
                process.stdout.close()
                process.stderr.close()
            if capture:
                capture.close()


if __name__ == "__main__":
    raise SystemExit(main())
