# Permission policy extension

`mpai-permissions` is an optional public `RuntimeExtension`. It handles interactive permission requests emitted by registered harnesses. The default mode is `Ask`.

```rust
use mpai::Runtime;
use mpai_permissions::{PermissionPolicyExtension, PolicyMode};

let runtime = Runtime::builder()
    // Register the harnesses the application uses.
    .extension(PermissionPolicyExtension::new(PolicyMode::AcceptEdits))
    .build();
```

| Mode | Behavior |
| --- | --- |
| `Ask` | Leave every request interactive. |
| `AcceptEdits` | Approve a known file/edit request only when a one-shot approval is offered. |
| `AutoApprove` | Approve recognized operation requests only when a one-shot approval is offered. |
| `Deny` | Select an offered `deny` or `deny_once` option for recognized operation requests. A missing one-operation denial remains interactive. |

File/edit kinds are exactly `file`, `edit`, `file_change`, `file_write`, and `patch`. Other recognized operations are `command`, `execute`, `read`, `search`, `fetch`, `tool`, and `mcp`. Questions, plans, broad permission grants, and unknown kinds remain interactive in every automatic mode. Classification uses the adapter's normalized `PermissionRequest.kind`; it does not guess from titles or branch on harness names.

A one-shot option has normalized kind `approve_once`, or generic kind `approve` with an offered ID of `approve`, `once`, `allow_once`, `approve_once`, `accept`, or `accept_once`. Unknown opaque IDs with generic approval kinds and duplicate selected IDs remain interactive. An adapter must preserve a session-wide approval as a distinct kind such as `approve_session`, rather than labeling it as a one-shot option. The extension never fabricates an option, selects a session-wide approval, submits question answers, or accepts a plan. The runtime sends the selected response through its normal permission validation and emits `permission.resolved`.

The extension contributes two commands through `extension/invoke`:

```json
{"extensionId":"permissions","command":"policy.get","arguments":{}}
{"extensionId":"permissions","command":"policy.set","arguments":{"mode":"accept_edits"}}
```

Mode values are `ask`, `auto_approve`, `accept_edits`, and `deny`. The setting is local to this extension instance and applies to subsequent requests across its runtime. Changing the setting does not resolve already pending requests or change provider configuration. Setting persistence belongs to the host application. The string parser additionally accepts CLI forms `auto-approve` and `accept-edits`.

This policy is not a provider sandbox or a universal tool restriction. Harnesses that execute operations without emitting a permission request cannot be constrained by this extension. Native permission modes, filesystem restrictions, command execution boundaries, and an adapter's accurate classification remain separate responsibilities. Installing `Ask` does not make an autoexecuting native provider interactive; installing `Deny` does not stop operations that bypass native permission prompts. Register at most one policy extension unless the host deliberately defines extension ordering; the runtime uses the first extension response.

Tests use the actual public `MockHarness` and runtime actor to prove one-shot edit approvals, client-resolvable pending command/question/plan requests, unknown option fallback, actual offered ID selection, and mode commands:

```sh
cargo test -p mpai-permissions
cargo clippy -p mpai-permissions --all-targets -- -D warnings
```
