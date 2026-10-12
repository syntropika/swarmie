#!/usr/bin/env python3
"""Linux/Unix SSH command fixture; it does not qualify actual SSH authentication."""
import json
import os
import sys

with open(os.environ["SWARMIE_SSH_FIXTURE_LOG"], "a", encoding="utf-8") as ledger:
    ledger.write(json.dumps({"pid": os.getpid(), "command": sys.argv[-1]}) + "\n")
if sys.argv[-1].startswith("command -v swarmie"):
    if os.environ.get("SWARMIE_SSH_FIXTURE_INSTALLED") == "0":
        sys.exit(127)
    print("swarmie 0.1.0")
    sys.exit(0)
os.execv("/bin/sh", ["sh", "-c", sys.argv[-1]])
