# Native known-cost usage GUI acceptance

This isolated fixture uses the existing resources GUI OS-input helper and an installed Pi harness against a deterministic local model endpoint. Explicit synthetic native model rates live in its private `models.json`: alpha input/output 20/40 USD per million tokens; beta 30/60. Each real native response reports 100 input and 10 output tokens. The fixture seeds ordinary project model defaults and a Git worktree, then leaves conversation creation, direct model selection, prompting, scope selection, group expansion, and dashboard refresh to actual Native SDK GUI input.

Run with an external, new artifact directory:

```sh
python3 tests/fixtures/usage-gui/acceptance.py --artifacts /absolute/external/artifacts/new-run
python3 tests/fixtures/usage-gui/verify.py --artifacts /absolute/external/artifacts/new-run
```

The controller accepts the existing resources GUI JSON-line actions and `restart-all`, which restarts both its daemon and native window. Save OS captures before and after restart. Send one alpha prompt on the main workspace and one beta prompt on the managed worktree, observe both scoped dashboards and All projects with By day, By harness, and By model expanded, restart, then send another beta prompt. Expected subtotals are 0.0024, 0.0036, 0.0060 before restart and 0.0096 overall afterward. Use read-only RPC to record exact native events and dashboard receipts; close both sessions and end stdin for process cleanup. The verified run's `actions.json` records the exact GUI sequence. No browser automation, CDP, DOM inspection, JavaScript evaluation, upstream authentication, or upstream billing claim is involved.

The verifier checks the complete evidence sequence, so it expects the same named captures as the recorded acceptance run. Research output and captures remain outside the repository. Do not print the synthetic endpoint credential; native configuration references its environment key only.
