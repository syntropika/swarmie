#!/usr/bin/env python3
"""Read-only deterministic GitHub CLI peer; mutations fail closed."""
import json
import os
import sys

with open(os.environ["SWARMIE_GUI_PEER_LEDGER"], "a", encoding="utf-8") as output:
    output.write(json.dumps({"peer": "gh", "argv": sys.argv[1:]}) + "\n")
args = sys.argv[1:]
if args[:2] == ["auth", "status"]:
    print("Fixture GitHub account is authenticated.")
elif args[:2] == ["pr", "view"]:
    print(json.dumps({"number": 39, "title": "Fixture pull request", "state": "OPEN", "url": "https://example.invalid/fixture/pull/39", "comments": [], "reviews": [], "statusCheckRollup": []}))
else:
    sys.exit("The desktop fixture permits read-only GitHub operations only.")
