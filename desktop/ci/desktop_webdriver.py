"""Real packaged Linux desktop under Xvfb; stdlib W3C WebDriver client, no renderer mock."""

import argparse
import http.client
import json
import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path
import importlib.util

# Isolated Python must not search the working directory for importable modules.
module = importlib.util.spec_from_file_location("owned_process", Path(__file__).with_name("run_owned.py"))
owned_process = importlib.util.module_from_spec(module)
module.loader.exec_module(owned_process)
group_alive, reap_group = owned_process.group_alive, owned_process.reap_group


class Driver:
    def __init__(self, port):
        self.port = port
        self.session = None

    def request(self, method, path, body=None):
        payload = None if body is None else json.dumps(body).encode("utf-8")
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=10)
        try:
            # urllib forces close; tauri-driver forwards it into its separate upstream pool.
            connection.request(method, path, body=payload,
                headers={"Content-Type": "application/json", "Connection": "keep-alive"})
            with connection.getresponse() as response:
                data = response.read(1024 * 1024 + 1)
                if len(data) > 1024 * 1024:
                    raise RuntimeError("Native WebDriver response exceeded its size limit.")
                if not 200 <= response.status < 300:
                    raise RuntimeError("Native WebDriver HTTP request failed.")
                value = json.loads(data)["value"]
        finally:
            connection.close()
        if isinstance(value, dict) and "error" in value:
            raise RuntimeError("Native WebDriver command failed.")
        return value

    def command(self, method, path, body=None):
        return self.request(method, f"/session/{self.session}" + path, body)

    def script(self, script, arguments=None):
        return self.command("POST", "/execute/sync", {"script": script, "args": arguments or []})

    def wait(self, condition, description):
        deadline = time.monotonic() + 10
        while True:
            result = condition()
            if result:
                return result
            if time.monotonic() >= deadline:
                raise TimeoutError(f"Native desktop readiness failed: {description}.")
            # Bounded DOM/driver state polling, not a fixed startup delay.
            time.sleep(0.03)

    def element(self, selector):
        value = self.command("POST", "/element", {"using": "css selector", "value": selector})
        return value["element-6066-11e4-a52e-4f735466cecf"]

    def click(self, selector):
        self.command("POST", f"/element/{self.element(selector)}/click", {})


def free_port():
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        return reservation.getsockname()[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--driver", type=Path, required=True)
    parser.add_argument("--native-close", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    args = parser.parse_args()
    if os.name == "nt" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise RuntimeError("This native GUI tier requires isolated hosted Linux/Xvfb.")
    args.work.mkdir(parents=True, exist_ok=False)
    owned_process.enable_reaping()
    env = os.environ.copy()
    env["NUMERA_ROOT"] = str(args.work)
    env.pop("NUMERA_DEVICE", None)  # Exercise the package's automatic CPU selection.
    env["GDK_BACKEND"] = "x11"
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("NUMERA_API_TOKEN", None)
    env.pop("NUMERA_ORT_LIBRARY", None)  # Exercise the installed desktop resource resolver.
    env.pop("NUMERA_WORKER_EXECUTABLE", None)
    port, native_port = free_port(), free_port()
    driver = Driver(port)
    owner = None
    with (args.work / "native-driver.log").open("wb") as log:
        try:
            owner = subprocess.Popen([str(args.driver), "--port", str(port), "--native-port", str(native_port)],
                env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
            def ready():
                if owner.poll() is not None:
                    raise RuntimeError("Owned native driver exited before readiness.")
                try:
                    with socket.create_connection(("127.0.0.1", native_port), timeout=0.2):
                        pass
                    return driver.request("GET", "/status") is not None
                except (OSError, http.client.HTTPException):
                    return False
            driver.wait(ready, "driver ports")
            capabilities = {"capabilities": {"alwaysMatch": {
                "browserName": "wry", "tauri:options": {"application": str(args.executable.resolve())}}}}
            driver.session = driver.request("POST", "/session", capabilities)["sessionId"]
            driver.command("POST", "/timeouts", {"implicit": 0, "script": 5000, "pageLoad": 10000})
            driver.wait(lambda: driver.script("return document.querySelector('#connection-state')?.textContent === 'Connected locally'"), "trusted IPC bootstrap")
            assert driver.script("return document.querySelector('h1').textContent") == "Evidence, not assumptions."
            driver.click("#recognize")
            driver.wait(lambda: driver.script("return document.querySelector('#result .answer')?.textContent === '36'"), "arithmetic answer")
            driver.wait(lambda: driver.script("return !!document.querySelector('#review-answer')"), "manual review")
            field = driver.element("#review-answer")
            driver.command("POST", f"/element/{field}/clear", {})
            driver.command("POST", f"/element/{field}/value", {"text": "37", "value": ["3", "7"]})
            button = driver.script("return Array.from(document.querySelectorAll('button')).find(x => x.textContent === 'I reviewed this answer')")
            driver.command("POST", f"/element/{button['element-6066-11e4-a52e-4f735466cecf']}/click", {})
            driver.wait(lambda: driver.script("return document.querySelector('#result').textContent.includes('Review confirmed. No answer was submitted to a website.')"), "explicit confirmation")
            assert driver.script("return document.querySelector('#result').textContent.includes('Server verified: no')")
            assert driver.script("return document.querySelector('#review-answer').value === '37' && document.querySelector('#review-answer').disabled")
            # WebKit closePage does not close the GTK window or exercise Tauri's exit event.
            executable = args.executable.resolve()
            applications = []
            for process in owned_process.psutil.Process(owner.pid).children(recursive=True):
                try:
                    if Path(process.exe()).resolve() == executable:
                        applications.append(process)
                except (owned_process.psutil.NoSuchProcess, owned_process.psutil.ZombieProcess):
                    pass
            if len(applications) != 1:
                raise RuntimeError("Expected exactly one owned packaged desktop process.")
            application = applications[0]
            birth = Path(f"/proc/{application.pid}/stat").read_text(encoding="utf-8").rsplit(") ", 1)[1].split()[19]
            if not application.is_running():
                raise RuntimeError("Owned desktop process ended before native close.")
            subprocess.run([str(args.native_close), str(application.pid), str(executable), birth],
                stdin=subprocess.DEVNULL, stdout=log, stderr=log, timeout=5, check=True)
            driver.wait(lambda: any("Desktop API and owned tasks drained." in p.read_text(encoding="utf-8") for p in (args.work / "logs").glob("*.log")), "owned application drain")
            application.wait(timeout=5)
            if application.is_running() and application.status() not in (
                    owned_process.psutil.STATUS_ZOMBIE, owned_process.psutil.STATUS_DEAD):
                raise RuntimeError("Owned desktop remained executing after its native close.")
            print("Packaged Linux native IPC/arithmetic/human-review/close passed.")
        except Exception:
            log.flush()
            for path in [args.work / "native-driver.log", *sorted((args.work / "logs").glob("*.log"))]:
                if path.is_file():
                    with path.open("rb") as capture:
                        capture.seek(max(0, path.stat().st_size - 8192))
                        text = capture.read(8192).decode("utf-8", errors="replace")
                    print("Native GUI diagnostic " + path.name + ": " + owned_process.safe(text), file=sys.stderr)
            raise
        finally:
            if driver.session:
                try:
                    driver.request("DELETE", f"/session/{driver.session}")
                except (OSError, http.client.HTTPException, RuntimeError):
                    pass  # Window may already have ended the session; outer process owner still reaps every descendant.
            if owner is not None:
                if group_alive(owner.pid):
                    os.killpg(owner.pid, signal.SIGTERM)
                try:
                    owner.wait(timeout=5)
                finally:
                    reap_group(owner)


if __name__ == "__main__":
    main()
