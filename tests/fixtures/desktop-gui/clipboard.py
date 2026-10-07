#!/usr/bin/env python3
"""Observe or seed only a display explicitly owned by the desktop fixture."""
import json
import os
import sys

display = os.environ.get("DISPLAY", "")
if display != os.environ.get("MPAI_GUI_PRIVATE_DISPLAY") or not display.startswith(":") or display in (":0", ":0.0", ""):
    raise SystemExit("Clipboard QA requires the fixture's explicit private X display.")

import gi
gi.require_version("Gdk", "4.0")
from gi.repository import Gdk, GLib

connection = Gdk.Display.open(display)
if connection is None:
    raise SystemExit("The owned display is unavailable.")
clipboard = connection.get_clipboard()
loop = GLib.MainLoop()
if sys.argv[1] == "seed":
    clipboard.set(sys.argv[2])
    print("CLIPBOARD_SEEDED=" + sys.argv[2], flush=True)
    loop.run()
elif sys.argv[1] == "read":
    state = {"completed": False}

    def done(source, result):
        try:
            print(json.dumps({"clipboardText": source.read_text_finish(result), "display": display}), flush=True)
        except Exception as error:
            print(json.dumps({"error": str(error), "display": display}), flush=True)
        state["completed"] = True
        loop.quit()

    def timeout():
        print(json.dumps({"error": "Private clipboard read timed out", "display": display}), flush=True)
        loop.quit()
        return False

    clipboard.read_text_async(None, done)
    GLib.timeout_add_seconds(4, timeout)
    loop.run()
    if not state["completed"]:
        raise SystemExit(1)
else:
    raise SystemExit("Use seed or read.")
