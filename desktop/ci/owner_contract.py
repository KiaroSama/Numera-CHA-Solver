"""Hosted owner regression includes children escaping the original process session."""

import argparse
import os
import socket
import subprocess
import sys
from pathlib import Path

import psutil


def child(mode, port):
    if mode == "idle":
        with socket.socket() as blocker:
            blocker.bind(("127.0.0.1", 0))
            blocker.listen(1)
            blocker.accept()
    elif mode in ("orphan", "detached"):
        child_process = subprocess.Popen([sys.executable, "-I", __file__, "--child", "idle"],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            start_new_session=mode == "detached")
        print(f"{child_process.pid} {psutil.Process(child_process.pid).create_time()}", flush=True)
        # Root exits immediately: discovery cannot rely on polling the parent before it exits.
    elif mode == "detached-live":
        child_process = subprocess.Popen([sys.executable, "-I", __file__, "--child", "idle"],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        print(f"{child_process.pid} {psutil.Process(child_process.pid).create_time()}", flush=True)
        with socket.socket() as blocker:
            blocker.bind(("127.0.0.1", 0))
            blocker.listen(1)
            blocker.accept()
    else:
        print("owned-success", flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--child", choices=("success", "idle", "orphan", "detached", "detached-live"))
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--work", type=Path)
    args = parser.parse_args()
    if args.child:
        child(args.child, args.port)
        return
    if not args.work or os.name == "nt":
        raise RuntimeError("POSIX owner regression requires an owned hosted work path.")
    args.work.mkdir(parents=True, exist_ok=False)
    import importlib.util
    contract = importlib.util.spec_from_file_location("numera_discovery_contract", Path(__file__).with_name("owner_discovery_contract.py"))
    module = importlib.util.module_from_spec(contract)
    contract.loader.exec_module(module)
    module.verify(args.work)
    owner = Path(__file__).with_name("run_owned.py")
    for mode, expected in (("success", 0), ("idle", 124), ("orphan", 1), ("detached", 1), ("detached-live", 124)):
        capture = args.work / f"{mode}.txt"
        try:
            result = subprocess.run([sys.executable, "-I", str(owner), "--cwd", str(args.work), "--work", str(args.work),
                "--wall", "5", "--idle", "2", "--capture", str(capture), "--", sys.executable, "-I", __file__, "--child", mode],
                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=12)
            assert result.returncode == expected, f"Owner mode={mode} returned={result.returncode}."
            if mode == "success":
                assert capture.read_text(encoding="utf-8").strip() == "owned-success"
            if mode in ("orphan", "detached", "detached-live"):
                pid, created = capture.read_text(encoding="utf-8").strip().split()
                try:
                    process = psutil.Process(int(pid))
                    assert not (process.is_running() and process.create_time() == float(created)), "Owned detached child survived cleanup."
                except psutil.NoSuchProcess:
                    pass
            assert not list(args.work.glob("owner-*.json"))
        finally:
            # A failed regression still owns its escaped child; never leave it for the outer group to miss.
            if mode in ("orphan", "detached", "detached-live") and capture.exists():
                content = capture.read_text(encoding="utf-8").strip().split()
                if len(content) == 2:
                    try:
                        process = psutil.Process(int(content[0]))
                        if process.is_running() and process.create_time() == float(content[1]):
                            process.kill()
                            process.wait(timeout=5)
                    except psutil.NoSuchProcess:
                        pass
    print("POSIX owner success/idle/orphan/detached-session cleanup regression passed.")


if __name__ == "__main__":
    main()
