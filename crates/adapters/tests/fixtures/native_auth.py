#!/usr/bin/env python3
"""Private authentication IPC fixture; never contacts a provider."""
import json
import sys
import time
import os

mode = sys.argv[1]
if mode == "google":
    if "--version" in sys.argv:
        print("1.3.5")
        sys.exit(0)
    assert os.isatty(0) and os.isatty(1)
    assert os.environ.get("SSH_CONNECTION")
    sys.stdout.write("Select login method:\n > 1. Google OAuth")
    sys.stdout.flush()
    assert sys.stdin.buffer.read(1) in (b"\r", b"\n")
    url = "https://accounts.google.com/o/oauth2/auth?state=fixture&scope=" + "a" * 200
    sys.stdout.write("\n\x1b]8;;" + url + "\x07Open authorization link\x1b]8;;\x07\n")
    sys.stdout.write("authorization code... ")
    sys.stdout.flush()
    code = sys.stdin.readline().strip()
    if code == "fixture-private-google-code":
        print("\nAuthentication successful!", flush=True)
    time.sleep(60)
elif mode == "console":
    print("Debug token: fixture-secret-do-not-publish", flush=True)
    print("\x1b[32mhttps://console.anthropic.com/oauth/authorize?state=fixture\x1b[0m", flush=True)
    sys.stdout.write("Paste authorization code: ")
    sys.stdout.flush()
    code = sys.stdin.readline().strip()
    if code == "fixture-private-code":
        print("Authentication successful", flush=True)
elif mode == "device":
    print("To sign in, open this URL in your browser:", file=sys.stderr, flush=True)
    print("https://auth.openai.com/codex/device", file=sys.stderr, flush=True)
    print("Then enter this code:", file=sys.stderr, flush=True)
    print("ABCD-EFGH", file=sys.stderr, flush=True)
    time.sleep(60)
elif mode == "overflow":
    sys.stdout.write("x" * 70000)
    sys.stdout.flush()
    time.sleep(60)
else:
    time.sleep(60)
