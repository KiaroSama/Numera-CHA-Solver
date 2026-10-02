import os
import socket
import threading

import pytest
import uvicorn
from playwright.sync_api import expect, sync_playwright

from numera_cha_solver.api import create_app
from numera_cha_solver.config import Settings


@pytest.mark.browser
@pytest.mark.timeout(45)
def test_owned_dashboard_authentication_arithmetic_and_mobile(tmp_path):
    if os.environ.get("NUMERA_TEST_BROWSER") != "1":
        pytest.skip("Owned headless browser tier was not selected.")
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    app = create_app(Settings(root=tmp_path, api_token="browser-test-only"))
    started = threading.Event()
    original = app.router.lifespan_context
    from contextlib import asynccontextmanager

    @asynccontextmanager
    async def lifespan(instance):
        async with original(instance):
            started.set()
            yield

    app.router.lifespan_context = lifespan
    server = uvicorn.Server(
        uvicorn.Config(app, host="127.0.0.1", port=port, log_level="error", access_log=False)
    )
    thread = threading.Thread(target=server.run, daemon=True)
    thread.start()
    try:
        assert started.wait(5), "Owned fixture server did not become ready."
        with sync_playwright() as playwright:
            browser = playwright.chromium.launch(headless=True)
            try:
                page = browser.new_page(viewport={"width": 1440, "height": 1000})
                page.set_default_timeout(5000)
                page.goto(f"http://127.0.0.1:{port}", wait_until="domcontentloaded", timeout=5000)
                expect(
                    page.get_by_role("heading", name="Evidence, not assumptions.")
                ).to_be_visible()
                page.get_by_role("button", name="Connect", exact=True).click()
                page.get_by_label("Local API credential").fill("browser-test-only")
                page.locator("#auth-form").get_by_role("button", name="Connect", exact=True).click()
                expect(page.locator("#connection-state")).to_have_text("Connected locally")
                page.get_by_role("button", name="Run recognition").click()
                expect(page.locator("#result .answer")).to_have_text("36")
                expect(page.locator("#result")).to_contain_text("Server verified: no")
                page.set_viewport_size({"width": 375, "height": 812})
                assert page.evaluate("document.documentElement.scrollWidth <= window.innerWidth")
                page.emulate_media(reduced_motion="reduce")
                assert (
                    page.locator("#recognize").evaluate(
                        "el => getComputedStyle(el).transitionDuration"
                    )
                    == "0s"
                )
            finally:
                browser.close()
    finally:
        server.should_exit = True
        thread.join(timeout=5)
        assert not thread.is_alive(), "Owned fixture server did not shut down."
