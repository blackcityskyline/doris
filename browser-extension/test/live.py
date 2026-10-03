#!/usr/bin/env python3
"""The add-on, in a real browser, clicking a real button.

    doris (bridge on)  <-  add-on background fetch  <-  content script button
                          on a page served off this machine

Everything else about the add-on is checked without a browser: the title
extraction under node, the manifest against the bridge's allow list in Rust.
What only a browser can answer is whether the button appears, whether its
click reaches the background script, and whether the fetch gets past CORS --
and that is this.

WebDriver is the only way to install a Firefox add-on temporarily and then
poke at the page it injected into. It needs three things this repository does
not carry:

    npm install --no-save geckodriver     # the driver
    a Firefox-based browser               # `browser` below
    doris running, with bridge_port set   # the thing under test

    python3 browser-extension/test/live.py [path-to-browser]

Every step prints what it saw, and the last line is only reached when every
one of them passed -- a summary printed from outside the checks is a summary
that can be printed without them.
"""

import http.server
import json
import os
import shutil
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
EXTENSION = os.path.abspath(os.path.join(HERE, ".."))
FIXTURE = os.path.join(HERE, "fixtures")
BROWSER = sys.argv[1] if len(sys.argv) > 1 else "/opt/zen-browser-bin/zen"

# The same packer the user installs with, so the file the check loads and the
# file on disk are one file and not two that happen to agree today.
sys.path.insert(0, os.path.dirname(HERE))
from pack import pack  # noqa: E402


def free_port():
    """A port nothing is listening on.

    Both ports, because a fixed one is how this check talks to somebody
    else's geckodriver instead of its own: the connection works, the steps
    pass, and the driver that was already running -- with a different
    profile, a different browser, whatever it had going -- is the one that
    answered. Ask the kernel for a port instead of claiming one.
    """
    import socket

    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


PORT = free_port()
PAGE = f"http://127.0.0.1:{PORT}/page.html"
DRIVER_PORT = free_port()
DRIVER = f"http://127.0.0.1:{DRIVER_PORT}"

# What the fixture page says, and therefore what the button must report.
# Written here rather than parsed out of the fixture so that editing the
# fixture does not quietly change what this asserts.
EXPECTED_TITLE = "Dune: Part Two"

BUTTON = (
    "const b = document.querySelector('.doris-search-button');"
    "return b ? {tag: b.tagName, text: b.textContent,"
    " title: b.dataset.dorisTitle || null} : null;"
)
SAID = (
    "const b = document.querySelector('.doris-search-button');"
    "return b ? {text: b.textContent, state: b.dataset.dorisState} : null;"
)


def fail(what, detail):
    print(f"FAIL  {what}\n      {detail}")
    sys.exit(1)


def say(step, text):
    print(f"  {step}. {text}")


# --- the driver -------------------------------------------------------------


def geckodriver():
    """The `geckodriver` binary: PATH, then the repository's node_modules.

    The repository's, because the documented install is
    `npm install --no-save geckodriver` run from the repository root, and
    that is where npm puts it.
    """
    repo = os.path.abspath(os.path.join(HERE, os.pardir, os.pardir))
    local = os.path.join(repo, "node_modules", ".bin", "geckodriver")
    found = shutil.which("geckodriver")
    if not found and os.path.exists(local):
        found = local
    if not found:
        print(
            "no geckodriver. Install one, from the repository root:\n"
            "    npm install --no-save geckodriver"
        )
        sys.exit(2)
    return found


def start_driver():
    process = subprocess.Popen(
        [geckodriver(), "--port", str(DRIVER_PORT), "--host", "127.0.0.1"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    for _ in range(40):
        try:
            with urllib.request.urlopen(f"{DRIVER}/status", timeout=2) as r:
                if json.loads(r.read())["value"]["ready"]:
                    return process
        except Exception:
            time.sleep(0.5)
    fail("geckodriver started", f"nothing answered on {DRIVER}")


# --- the page ---------------------------------------------------------------


def serve_fixture():
    handler = lambda *a, **k: http.server.SimpleHTTPRequestHandler(*a, directory=FIXTURE, **k)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", PORT), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


# --- WebDriver --------------------------------------------------------------


def call(method, path, body=None, timeout=90):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(
        DRIVER + path, data=data, method=method,
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
    except urllib.error.HTTPError as e:
        fail(f"{method} {path}", f"HTTP {e.code}: {e.read().decode(errors='replace')[:400]}")
    return json.loads(raw) if raw else {}


def js(session, script):
    return call("POST", f"/session/{session}/execute/sync", {"script": script, "args": []})["value"]


def poll(session, script, settled, tries=40, pause=0.5):
    """Run a snippet until `settled` says so, or give up and return the last.

    A page that was told to load something answers "not yet" for a while, and
    a button that has just been clicked answers "sending…" first. Both need a
    wait, and a sleep with no exit condition is a test that passes on timing.
    """
    last = None
    for _ in range(tries):
        last = js(session, script)
        if settled(last):
            return last
        time.sleep(pause)
    return last


def start_session():
    answer = call("POST", "/session", {"capabilities": {"alwaysMatch": {
        "browserName": "firefox",
        "moz:firefoxOptions": {
            "binary": BROWSER,
            "args": ["-headless"],
            "prefs": {
                # No first-run pages and no updates: every one of them can
                # eat a click meant for the button.
                "browser.shell.checkDefaultBrowser": False,
                "browser.startup.homepage_override.mstone": "ignore",
                "datareporting.policy.dataSubmissionEnabled": False,
                "app.update.enabled": False,
            },
        },
    }}})
    session = answer["value"]["sessionId"]
    say(1, f"browser {session[:12]}… on {BROWSER}")
    return session


def main():
    serve_fixture()
    driver = start_driver()
    archive = pack()[0]
    session = None
    try:
        session = start_session()
        installed = call("POST", f"/session/{session}/moz/addon/install",
                         {"path": archive, "temporary": True})
        say(2, f"add-on installed: {installed['value']}")

        call("POST", f"/session/{session}/url", {"url": PAGE})
        button = poll(session, BUTTON, lambda v: bool(v))
        if not button:
            state = js(session, "return {url: location.href, ready: document.readyState,"
                                " h1: !!document.querySelector('h1')};")
            fail("the button was injected", f"the page says {state}")
        say(3, f"button injected: <{button['tag']}> “{button['text']}”, "
              f"title {button['title']!r}")
        if button["title"] != EXPECTED_TITLE:
            fail("the title it read", f"{button['title']!r}, wanted {EXPECTED_TITLE!r}")

        ref = call("POST", f"/session/{session}/elements",
                   {"using": "css selector", "value": ".doris-search-button"}
                   )["value"][0]["element-6066-11e4-a52e-4f735466cecf"]
        call("POST", f"/session/{session}/element/{ref}/click", {})

        said = poll(session, SAID,
                    lambda v: bool(v) and v["state"] in ("ok", "error"))
        if not said:
            fail("the button reported anything", "it never left “sending…”")
        say(4, f"after the click: “{said['text']}” (state {said['state']})")
        if said["state"] != "ok":
            fail("doris took the title", said["text"])
    finally:
        if session:
            try:
                call("DELETE", f"/session/{session}")
            except SystemExit:
                raise
            except Exception:
                pass
        driver.terminate()
    print("\nlive: page -> button -> background fetch -> bridge -> doris")
    print("now look at doris: the query is in its search box.")


if __name__ == "__main__":
    main()