# Execution policy

Clients select an execution profile through `config.executionProfile` at creation or `agent/configure`. The host and adapter compile that intent into native configuration. Clients consume the server's choices, mapping, availability, application scope and reasons; they do not translate native permission settings.

## Common profiles

| Profile | Contract |
| --- | --- |
| `ask` | Use the supported native approval checks and leave unresolved requests to the user. Ordinary allowed operations may run without a prompt. |
| `review` | Use a qualified native reviewer for eligible operation requests. Unresolved requests remain interactive. |
| `full` | Request the native broad execution route without native approval prompts, within operating-system and administrative permissions. |

Each advertised choice has an `equivalent`, `partial`, `unsupported` or `unknown` mapping. Only an available equivalent mapping can be selected as a common profile. Disabling prompts alone does not establish broad filesystem or network access. An extension dialog or question channel does not establish operation-approval coverage. `capabilities.operationApprovals` records that coverage independently of the general interactive `permissions` capability.

Native controls remain available through configuration descriptors. An explicit change to a controlled native setting clears the retained common intent. It can produce a custom policy even when some individual settings resemble a common mode. Structured native policies retain their complete fields; unfamiliar modes are not replaced with restrictive or permissive defaults. `native_custom` describes a native observation when an adapter supplies it; it is not a portable profile that clients can request.

## Intent and evidence

`SessionSnapshot.executionPolicy`, configuration receipts and previews carry the same policy contract:

- `requested` and `revision` retain the user's common selection.
- `planned` and `plannedProfile` describe the candidate native configuration.
- `observed` retains native values, evidence, coverage, limitations and an observed native version when available.
- `bindingId` and `nativeSession` identify the execution context and native connection to which the evidence belongs.
- `controlledSettings` identifies native alternatives for generic client presentation.
- `dispatchBlocked` and `reason` preserve an unresolved application or intent mismatch.

| Status | Meaning |
| --- | --- |
| `staged` | The candidate is prepared for a later turn and differs from the current observation. |
| `acknowledged` | The native endpoint acknowledged the submitted configuration; this is not independent observation of every executed operation. |
| `effective` | A native configuration receipt matches the candidate. This is evidence about the reported controls, not an operating-system isolation certificate. |
| `partial` | A failed mutation changed some observed native controls. |
| `unknown` | Application or current evidence is insufficient. |
| `rejected` | Reserved for adapters that can establish an explicit native rejection. |

A client must show selected future configuration separately from current native observations. No observation means unknown coverage, not an inferred common mode. Available labels and descriptions come from the server.

## Applying and recovering configuration

For an existing session, before a native policy mutation, the host persists intent and a dispatch guard. Native calls run outside storage transactions. The result and corresponding event commit together. A partial application, failed observation or failed result write retains uncertainty and blocks policy-dependent execution. A successful unrelated configuration update or ordinary readback cannot release that guard.

An explicit supported policy selection can repair the state. Resume recompiles retained common intent for the new connection. Prior unresolved application remains blocked unless an exact effective native receipt reconciles it; a native acknowledgement alone cannot erase retained uncertainty. Replacing a native session clears stale observations and pending requests, and requalifies retained intent. If the replacement occurs during an active turn that cannot accept configuration, explicit repair remains necessary once it is idle.

New prompt dispatch, queued dispatch, steering, compaction and rollback respect the guard. Offered denials and cancellation remain available. Permission changes require a scope supported by the adapter and do not replace a setting while a request is pending.

## Host permission authority

At most one registered extension may declare `RuntimeExtension::permission_authority()`. Native connection and configuration preview reject multiple authorities before native effects. Only this authority receives the decision hook. Other extensions can observe permission events through `on_event`.

An authority returning no response or an error leaves the request unresolved. There is no fallback to another approving extension. While `ask` or `review` is explicitly selected, the host rejects automatic extension approvals for unresolved native requests. Offered denials remain valid, and the user can explicitly answer through the normal client protocol. A runtime-wide extension mode cannot silently replace per-session common intent.

This authority applies only to requests actually surfaced by the adapter. It does not confine trusted extension code or stop operations that bypass native requests.

## Current limits and later work

This slice implements the permission-profile contract and existing native configuration translation. Common support is intentionally uneven where native coverage cannot establish equivalence. The desktop exposes scalar native choices and preserves structured custom values without automatic rewriting.

The bounded host-tool broker supplies a context-bound read through supported MCP adapters with explicit per-session grants and turn cancellation. See [extension tool delivery](extension-api.md#agent-facing-tool-delivery). Broader tools, direct native extension delivery, complete native-release qualification, version-specific adaptation fixtures, advanced filesystem isolation, collaboration and cloud allocation remain separate work. Native fixture tests establish protocol behavior; they do not certify a live provider release, model execution or operating-system confinement.
