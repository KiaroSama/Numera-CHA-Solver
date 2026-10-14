"""Owned headless dashboard regression for the native server; test tooling only."""

import argparse
import logging
import time
import os
import socket
import subprocess
import uuid
from pathlib import Path

from playwright.sync_api import expect, sync_playwright


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    work = args.work.resolve()
    if not work.is_relative_to(root) or work in {root, root / "data"}:
        raise ValueError("Browser fixture must use isolated project working state.")
    work.mkdir(parents=True, exist_ok=True)
    log_directory = work / "logs"
    log_directory.mkdir(exist_ok=True)
    logger = logging.getLogger("native_browser")
    logger.setLevel(logging.INFO)
    try:
        handler = logging.FileHandler(log_directory / f"native-browser_{time.strftime('%Y-%m-%d_%H-%M-%S', time.gmtime())}_UTC_{uuid.uuid4().hex[:8]}.log", encoding="utf-8")
    except OSError:
        handler = logging.StreamHandler()
    formatter = logging.Formatter("[%(asctime)s UTC] [%(levelname)s] [browser] %(message)s", datefmt="%Y-%m-%d %H:%M:%S")
    formatter.converter = time.gmtime
    handler.setFormatter(formatter)
    logger.addHandler(handler)
    logger.info("Owned headless native dashboard verification started.")
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    env = os.environ.copy()
    env["NUMERA_ROOT"] = str(work)
    env["NUMERA_API_TOKEN"] = "browser-owned-test-only"
    child = None
    try:
        child = subprocess.Popen([str(args.executable.resolve()), "serve", "--port", str(port)],
            env=env, cwd=work, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        import urllib.request
        deadline = time.monotonic() + 5
        while True:
            if child.poll() is not None:
                raise RuntimeError("Owned native dashboard exited before readiness.")
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{port}/health/live", timeout=0.5) as response:
                    if response.status == 200:
                        break
            except OSError:
                if time.monotonic() >= deadline:
                    raise TimeoutError("Owned dashboard readiness deadline exhausted.") from None
                time.sleep(0.02)
        with sync_playwright() as playwright:
            browser = playwright.chromium.launch(headless=True)
            try:
                page = browser.new_page(viewport={"width": 1440, "height": 1000})
                page.set_default_timeout(5000)
                page.goto(f"http://127.0.0.1:{port}", wait_until="domcontentloaded", timeout=5000)
                expect(page.get_by_role("heading", name="Evidence, not assumptions.")).to_be_visible()
                page.get_by_role("button", name="Connect", exact=True).click()
                page.get_by_label("Local API credential").fill("browser-owned-test-only")
                page.locator("#auth-form").get_by_role("button", name="Connect", exact=True).click()
                expect(page.locator("#connection-state")).to_have_text("Connected locally")
                page.get_by_role("button", name="Run recognition").click()
                expect(page.locator("#result .answer")).to_have_text("36")
                expect(page.locator("#result")).to_contain_text("Server verified: no")
                first_review = page.evaluate("review")
                page.route("**/v1/tasks", lambda route: route.abort() if route.request.method == "POST" else route.continue_())
                page.get_by_role("button", name="Run recognition").click()
                expect(page.get_by_role("button", name="I reviewed this answer")).to_be_disabled()
                assert page.evaluate("review === null")
                old_state = page.evaluate("async id => (await api(`/v1/reviews/${id}`)).state", first_review["id"])
                assert old_state == "cancelled", "Old observation review remained actionable."
                page.unroute("**/v1/tasks")
                page.get_by_role("button", name="Run recognition").click()
                expect(page.get_by_label("Review and correct this suggestion")).to_be_enabled()
                page.get_by_label("Review and correct this suggestion").fill("37")
                page.get_by_role("button", name="I reviewed this answer").click()
                expect(page.locator("#result")).to_contain_text("Review confirmed. No answer was submitted to a website.")
                page.set_viewport_size({"width": 375, "height": 812})
                assert page.evaluate("document.documentElement.scrollWidth <= window.innerWidth")
                page.emulate_media(reduced_motion="reduce")
                assert page.locator("#recognize").evaluate("el => getComputedStyle(el).transitionDuration") == "0s"
                page.screenshot(path=str(work / "native-dashboard.png"))
            finally:
                browser.close()
        logger.info("Authentication/arithmetic/mobile/reduced-motion verified.")
        print("Native dashboard authentication/arithmetic/mobile/reduced-motion verified.")
        return 0
    except Exception:
        logger.error("Dashboard verification failed; sensitive diagnostic details suppressed.")
        raise
    finally:
        try:
            if child is not None:
                if child.poll() is None:
                    child.terminate()
                child.wait(timeout=5)
        finally:
            logger.info("Owned browser/server cleanup finished.")
            logger.removeHandler(handler)
            handler.close()


if __name__ == "__main__":
    raise SystemExit(main())
