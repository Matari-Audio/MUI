#!/usr/bin/env python3
"""Exercise the real WASM worker and canvas in CI; retain browser evidence."""
import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
from threading import Thread
import time

from playwright.sync_api import sync_playwright


def frame(page):
    return page.locator("#canvas").evaluate("""async canvas => {
      const bytes = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
      const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
      const colors = new Set();
      for (let i = 0; i < bytes.length; i += 4) colors.add(bytes.slice(i, i + 4).join(','));
      return {hash: Array.from(digest, n => n.toString(16).padStart(2, '0')).join(''),
              colors: colors.size, width: canvas.width, height: canvas.height};
    }""")


def rendered(page):
    page.wait_for_function("document.querySelector('#status').textContent.includes(' draws ')")
    assert not page.locator("#error").is_visible(), "worker reported a rendering error"
    result = frame(page)
    assert result["colors"] > 1, "worker canvas is blank or uniform"
    assert (result["width"], result["height"]) == (640, 480)
    return result


def check(page, output):
    evidence = {"starter": rendered(page)}
    for example in ("regions", "cut", "layout", "icons", "weld"):
        page.locator(f'[data-example="{example}"]').click()
        # Clicking loads an example asynchronously; wait for its worker result,
        # rather than accepting the previous successful canvas.
        page.wait_for_function("name => document.querySelector(`[data-example='${name}']`).getAttribute('aria-pressed') === 'true'", arg=example)
        evidence[example] = rendered(page)
    page.locator("#source").fill("block(100., 100.).fill(Primary)")
    page.locator("#run").click()
    first = rendered(page)
    page.locator("#source").fill("block(100., 100.).fill(Secondary)")
    page.locator("#run").click()
    second = rendered(page)
    assert first["hash"] != second["hash"], "editing the scene did not change pixels"
    evidence.update(primary=first, secondary=second)
    page.locator("#source").fill("block(2.)")
    page.locator("#run").click()
    page.locator("#error").wait_for(state="visible")
    page.locator("#reset").click()
    evidence["recovered"] = rendered(page)
    assert evidence["recovered"]["hash"] == evidence["weld"]["hash"], "reset did not restore the selected example"
    for width in (1280, 360):
        page.set_viewport_size({"width": width, "height": 900})
        bounds = page.locator("#canvas").bounding_box()
        assert bounds and bounds["width"] > 0 and bounds["height"] > 0
        assert bounds["x"] >= 0 and bounds["x"] + bounds["width"] <= width + 1
        assert frame(page)["hash"] == evidence["recovered"]["hash"], "viewport resize lost canvas pixels"
        page.screenshot(path=str(output / f"viewport-{width}.png"), full_page=True)
    return evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--browser", choices=("chromium", "firefox", "webkit"), required=True)
    parser.add_argument("--site", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    result = {"status": "failed", "browser": args.browser}
    errors, console = [], []
    started = time.monotonic()
    with ThreadingHTTPServer(("127.0.0.1", 0), partial(SimpleHTTPRequestHandler, directory=str(args.site.resolve()))) as server:
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with sync_playwright() as playwright:
                browser = getattr(playwright, args.browser).launch()
                context = browser.new_context(viewport={"width": 1280, "height": 900})
                context.tracing.start(screenshots=True, snapshots=True, sources=True)
                page = context.new_page()
                page.set_default_timeout(30000)
                page.on("pageerror", lambda error: errors.append(str(error)))
                page.on("console", lambda message: console.append({"type": message.type, "text": message.text}))
                result["version"] = browser.version
                try:
                    page.goto(f"http://127.0.0.1:{server.server_port}/")
                    result["frames"] = check(page, args.output)
                    assert not errors, errors
                    result["status"] = "passed"
                except Exception:
                    page.screenshot(path=str(args.output / "failure.png"), full_page=True)
                    raise
                finally:
                    context.tracing.stop(path=str(args.output / "trace.zip"))
                    browser.close()
        except Exception as error:
            result["error"] = str(error)
        finally:
            server.shutdown()
            thread.join()
    result.update(elapsed_seconds=time.monotonic() - started, page_errors=errors, console=console)
    (args.output / "result.json").write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
