#!/usr/bin/env python3
"""Exercise actual native window context and file inputs in an owned X11 session."""
import argparse
import base64
import hashlib
import importlib.util
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import time

sys.dont_write_bytecode = True
fixture = Path(__file__).resolve()
spec = importlib.util.spec_from_file_location("desktop_fixture", fixture.parents[1] / "desktop-gui/acceptance.py")
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)


class Platform(native.Desktop):
    def __init__(self, root, artifacts, media):
        super().__init__(root, artifacts, media)
        vendor = fixture.parents[1] / "native-os/vendor"
        ocr = fixture.parents[1] / "resources-gui/vendor"
        self.env.update(
            PATH=str(vendor / "bin") + ":" + str(ocr / "usr/bin") + ":" + self.env["PATH"],
            LD_LIBRARY_PATH=str(vendor / "usr/lib/x86_64-linux-gnu") + ":" + str(ocr / "usr/lib/x86_64-linux-gnu"),
            PYTHONPATH=str(vendor / "usr/lib/python3/dist-packages"),
            XDG_DATA_DIRS=str(vendor / "usr/share") + ":/usr/local/share:/usr/share",
            TESSDATA_PREFIX=str(ocr / "usr/share/tesseract-ocr/5/tessdata"),
            MAGICK_CONFIGURE_PATH=str(vendor / "etc/ImageMagick-6"),
            MAGICK_CODER_MODULE_PATH=str(vendor / "usr/lib/x86_64-linux-gnu/ImageMagick-6.9.12/modules-Q16/coders"),
            GTK_A11Y="atspi", NO_AT_BRIDGE="0", WEBKIT_DISABLE_COMPOSITING_MODE="1",
        )
        self.xvfb = self.child(["Xvfb", "-displayfd", "1", "-screen", "0", "2200x1200x24", "-nolisten", "tcp"], pipe=True)
        self.display = ":" + self.ready(self.xvfb, "")
        self.env.update(DISPLAY=self.display, MPAI_OS_PRIVATE_DISPLAY=self.display)
        self.child([str(vendor / "bin/openbox"), "--config-file", str(vendor / "etc/xdg/openbox/rc.xml")], log="openbox.log")
        time.sleep(.7)
        self.selected = self.child(["/usr/bin/python3", str(fixture.parents[1] / "native-os/window.py"), "selected"], log="selected.log")
        self.decoy = self.child(["/usr/bin/python3", str(fixture.parents[1] / "native-os/window.py"), "decoy"], log="decoy.log")
        time.sleep(.8)
        windows = subprocess.check_output([str(vendor / "bin/wmctrl"), "-lp"], env=self.env, text=True)
        selected = next(line.split()[0] for line in windows.splitlines() if "MPAI controlled native OS fixture" in line)
        subprocess.run([str(vendor / "bin/wmctrl"), "-ir", selected, "-e", "0,1500,80,-1,-1"], env=self.env, check=True)
        self.selected_window = selected

    def child(self, command, **kwargs):
        if command[0] == "Xvfb" and hasattr(self, "xvfb"):
            # The base helper normally creates its display after the daemon. Reuse
            # ours so the daemon's OS integrations share this exact private display.
            return self.xvfb
        return super().child(command, **kwargs)

    def ready(self, process, prefix, timeout=25):
        if hasattr(self, "xvfb") and process is self.xvfb and hasattr(self, "display"):
            return self.display.removeprefix(":")
        return super().ready(process, prefix, timeout)

    def launch_host(self):
        # All owned programs inherit the same fixture-owned accessibility bus.
        self.host = self.child([str(self.root / "apps/desktop/zig-out/bin/mpai-desktop")], log="host.log", cwd=self.root / "apps/desktop")
        deadline = time.monotonic() + 50
        while time.monotonic() < deadline:
            result = subprocess.run(["xdotool", "search", "--onlyvisible", "--pid", str(self.host.pid), "--name", "^mpai$"], env=self.env, capture_output=True, text=True)
            if result.returncode == 0:
                self.window = result.stdout.splitlines()[-1]
                self.x("windowsize", self.window, 1280, 900)
                self.x("windowmove", self.window, 0, 0)
                self.x("windowactivate", "--sync", self.window)
                time.sleep(3)
                return
            if self.host.poll() is not None:
                raise RuntimeError("Native host exited; inspect host.log")
            time.sleep(.1)
        (self.artifacts / "window-inventory.txt").write_text(subprocess.check_output(["xwininfo", "-root", "-tree"], env=self.env, text=True))
        raise TimeoutError("Native window visibility")

    def capture(self, name):
        if not name.replace("-", "").replace("_", "").isalnum():
            raise ValueError("Use a simple capture name")
        time.sleep(.4)
        information = subprocess.check_output(["xwininfo", "-id", self.window], env=self.env, text=True)
        def field(label):
            return int(re.search(re.escape(label) + r":\s+(-?\d+)", information).group(1))
        geometry = {"X": field("Absolute upper-left X"), "Y": field("Absolute upper-left Y"),
                    "WIDTH": field("Width"), "HEIGHT": field("Height"), "WINDOW": self.window}
        target = self.artifacts / (name + ".png")
        subprocess.run(["ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-f", "x11grab", "-video_size",
                        str(geometry["WIDTH"]) + "x" + str(geometry["HEIGHT"]), "-i",
                        self.display + "+" + str(geometry["X"]) + "," + str(geometry["Y"]), "-frames:v", "1", str(target)],
                       env=self.env, check=True)
        ocr = subprocess.run([str(fixture.parents[1] / "resources-gui/vendor/usr/bin/tesseract"), str(target), "stdout"],
                             env=self.env, capture_output=True, text=True, check=True).stdout
        target.with_suffix(".txt").write_text(ocr)
        return {"screenshot": str(target), "geometry": geometry, "ocr": ocr}

    def action(self, request):
        if request["action"] == "assert-prompt":
            users = [event for event in map(json.loads, (self.artifacts / "sessions" / (self.session + ".jsonl")).read_text().splitlines())
                     if event.get("type") == "message.user"]
            event = users[-1]
            parts = event["data"]["content"]
            text = "\n".join(part["data"] for part in parts if part["type"] == "text")
            for value in request.get("contains", []):
                assert value in text, "Expected submitted context: " + value
            for value in request.get("excludes", []):
                assert value not in text, "Unexpected submitted context: " + value
            images = [part for part in parts if part["type"] == "image"]
            assert len(images) == request.get("images", 0), "Actual dispatched image count differs"
            result = {"passed": True, "event": event}
            (self.artifacts / (request["name"] + ".json")).write_text(json.dumps(result, indent=2) + "\n")
            return result
        if request["action"] == "inspect-assets":
            results = []
            for path in sorted((self.artifacts / "sessions/extensions/workspace").rglob("*")):
                if path.is_file():
                    results.append({"path": str(path.relative_to(self.artifacts)), "bytes": path.stat().st_size,
                                    "sha256": hashlib.sha256(path.read_bytes()).hexdigest()})
            (self.artifacts / (request.get("name", "assets") + ".json")).write_text(json.dumps(results, indent=2) + "\n")
            return results
        return super().action(request)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=fixture.parents[3])
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--media", type=Path)
    args = parser.parse_args()
    root, out = args.root.resolve(), args.artifacts.resolve()
    if out == root or root in out.parents or out.exists():
        parser.error("A fresh artifact directory outside the project is required")
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS"):
        raise SystemExit("Run this fixture inside its own dbus-run-session")
    desktop = None
    try:
        desktop = Platform.__new__(Platform)
        desktop.__init__(root, out, args.media)
        print(json.dumps({"ready": desktop.start(), "selectedWindow": desktop.selected_window}), flush=True)
        for line in sys.stdin:
            if not line.strip():
                continue
            request = json.loads(line)
            try:
                response = {"request": request, "result": desktop.action(request)}
            except Exception as error:
                response = {"request": request, "error": str(error)}
            with (out / "actions.jsonl").open("a") as file:
                file.write(json.dumps(response) + "\n")
            print(json.dumps(response), flush=True)
    finally:
        if desktop:
            desktop.close()


if __name__ == "__main__":
    main()
