"""Hosted-only real-socket contracts for native WebDriver transport, without a GUI."""

import http.client
import importlib.util
import json
import os
import socket
import threading
from pathlib import Path

module = importlib.util.spec_from_file_location("native_driver", Path(__file__).with_name("desktop_webdriver.py"))
native_driver = importlib.util.module_from_spec(module)
module.loader.exec_module(native_driver)


def main():
    if os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise RuntimeError("Native transport contracts require hosted CI.")
    answer = {"answer": "فارسی"}
    cases = [
        (200, json.dumps({"value": answer}, ensure_ascii=False).encode("utf-8"), None),
        (503, b'{"value":null}', RuntimeError),
        (200, b'{"value":{"error":"controlled"}}', RuntimeError),
        (200, b"x" * (1024 * 1024 + 1), RuntimeError),
        (200, b"not-json", ValueError),
        (None, b"", http.client.RemoteDisconnected),
    ]
    failures = []
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        listener.settimeout(2)
        driver = native_driver.Driver(listener.getsockname()[1])
        driver.session = "controlled"

        def serve():
            try:
                for status, body, _ in cases:
                    connection, _ = listener.accept()
                    with connection:
                        connection.settimeout(2)
                        request = bytearray()
                        while b"\r\n\r\n" not in request:
                            chunk = connection.recv(4096)
                            assert chunk, "Client ended before its request headers."
                            request.extend(chunk)
                            assert len(request) <= 8192, "Unexpected request size."
                        header, payload = bytes(request).split(b"\r\n\r\n", 1)
                        lines = header.decode("ascii").split("\r\n")
                        assert lines[0] == "POST /session/controlled/execute/sync HTTP/1.1"
                        headers = dict(line.lower().split(": ", 1) for line in lines[1:])
                        assert headers["connection"] == "keep-alive", "Close must not reach the upstream pool."
                        length = int(headers["content-length"])
                        assert 0 < length <= 1024
                        while len(payload) < length:
                            chunk = connection.recv(length - len(payload))
                            assert chunk, "Client ended before its request body."
                            payload += chunk
                        assert json.loads(payload) == {"script": "return 1", "args": []}
                        if status is None:
                            continue  # An uncertain command must fail, never be repeated.
                        connection.sendall(f"HTTP/1.1 {status} Controlled\r\nContent-Length: {len(body)}\r\nContent-Type: application/json\r\n\r\n".encode("ascii") + body)
                        assert connection.recv(1) == b"", "Client socket leaked after its response."
            except BaseException as error:
                failures.append(error)

        thread = threading.Thread(target=serve)
        thread.start()
        try:
            for _, _, expected_error in cases:
                if expected_error is None:
                    assert driver.script("return 1") == answer
                else:
                    try:
                        driver.script("return 1")
                    except expected_error:
                        pass
                    else:
                        raise AssertionError("Native transport accepted a controlled failure.")
            thread.join(timeout=3)
            assert not thread.is_alive(), "Owned wire fixture did not finish."
            if failures:
                raise failures[0]
            listener.setblocking(False)
            try:
                extra, _ = listener.accept()
            except BlockingIOError:
                pass
            else:
                extra.close()
                raise AssertionError("Native transport retried an uncertain command.")
        finally:
            listener.close()
            thread.join(timeout=3)
            if thread.is_alive():
                raise RuntimeError("Owned transport fixture thread survived cleanup.")
            if failures:
                raise AssertionError("Owned transport server contract failed.") from failures[0]
    print("Native transport keepalive, client close, UTF-8, HTTP/tool errors, size, malformed JSON and no-retry contracts passed.")


if __name__ == "__main__":
    main()
