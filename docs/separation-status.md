# Native application and presentation separation

The native client owns node credentials, reconnect supervision, durable prompt intent, delivery recovery, and verified session hydration. The independently running runtime owns Harness execution, canonical events, execution permissions, and its authoritative submission ledger.

## Native application operations

`client/sessionInput` accepts authored prompt, queue and steering intent. Native code resolves the destination session, validates attachments and selected context, prepares provider content and retained assets, and preserves the original authored metadata. Target-qualified `session/prompt` requests use the same submission journal; raw runtime requests without an exact target retain the runtime dispatcher contract. The gateway verifies the exact node identity and runtime incarnation, retains intent before dispatch, and persists the resulting delivery knowledge in its client SQLite store. These records are independent of the replaceable view cache. Reusing a key returns its receipt without redispatching input; changing its input is rejected. An unresolved original input blocks another prompt in the same session. Missing runtime evidence never proves rejection.

Per-session operating-system file locks prevent concurrent client processes from bypassing the guard. A separate journal file lock protects retained record updates across processes. Lock acquisition uses nonblocking checks with a five-second bound. Background observation skips busy sessions and queries receipts without resending prompts.

`client/submissions` returns retained receipts, unresolved status, and an authoritative session snapshot. The caller supplies the exact expected runtime. Explicit reconciliation can query the Harness through the runtime; routine observation never creates a native session.

`client/sessionProjection` returns the same projection plus durable transcript events. Native code pages records in groups of 256 through the captured snapshot sequence, verifies session identity and contiguous ordering, and rejects incomplete history. The total JSON projection is limited to 48 MiB, including receipts and snapshot, within the native IPC transfer budget. The view applies presentation reducers to this verified event range.

## Session and project workflows

The native client exposes application operations through a shared dispatcher used by both the native IPC gateway and authenticated browser HTTP adapters. Application intents use correlated requests with named parameters. Raw runtime batches, notifications and malformed envelopes retain canonical JSON-RPC handling. A bare runtime without desktop assets keeps its runtime interface. Application operations require the exact retained node identity and runtime incarnation:

| Intent | Native operation and ownership |
| --- | --- |
| Prepare or create a session | `client/sessionPrepare` and `client/sessionCreate` resolve launch intent on the destination node, verify project revision and worktree membership, and use the runtime's selected projectless directory. |
| Submit authored input | `client/sessionInput` prepares text, selected context, skills and attachments on the destination node. Prompt intent and metadata survive view-cache replacement; queue and steering metadata do not prove execution. |
| Change history or recover delivery | `client/sessionHistory` recovers resources before fork, branch, edit or rollback. It uses canonical history and an actor-level sequence guard for rollback. `client/sessionRecover` reconciles delivery before deciding whether attachment is permitted. |
| Change queued input | `client/sessionQueue` reads the authoritative queue, preserves nontext resources during edits and retains updated authored metadata. |
| Open a conversation | `client/sessionOpen` recovers retained receipts and verified transcript events, decides whether attachment is permitted, and returns activation status and the attached queue. |
| Read or change controls | `client/sessionControls` composes advertised settings, models, observed values and execution policy. `client/sessionConfigure` attempts the update once and returns observed controls inside an error after a partial failure. |
| Open or list projects | `client/projectOpen`, `client/projectGet` and `client/projectCatalog` retain canonical project identity and names, compose Git or ordinary-folder views, and preserve unavailable records. |
| Create projects and import conversations | `client/projectCreate` owns folder creation or clone followed by registration. `client/sessionImport` imports and opens the retained session. Partial failures retain created resource identity. |
| Built-in extension workflows | `client/gitText` prepares and parses generated drafts; `client/prProjection` aggregates review data; `client/comparison` resolves launch directories and creates requested worktrees; `client/schedulePut` validates scheduling intent and creates a session when required. |
| Save files | `client/fileSave` checks the observed baseline before writing. This detects an already changed file but is a read-then-write check, not an operating-system atomic compare-and-swap. |
| Resolve directory and defaults | `client/chatDirectory` and `client/projectDefaults` compose node-owned directory records and revision-guarded settings. |
| Invoke extensions and manage worktrees | `client/extensionInvoke` translates retained project identity for Git commands. `client/worktree` composes one explicit mutation with refreshed worktrees and retains created-tree details on a partial failure. |

Forks retain authored metadata only for active user messages represented in the child canonical history, with child sequence anchors. Editing recovers images, selected skills and retained binary assets before mutation. Missing resources block the change; created sessions and recovered drafts remain in partial-error data. Rollback checks the expected sequence inside the execution actor, immediately before provider dispatch. Fork attachment and authoritative runtime capability checks remain runtime responsibilities.

Every workflow captures the full runtime identity before execution and retains that exact target for its subsequent calls. A reconnect cannot redirect an in-flight operation. Mutations are not retried automatically. Errors after earlier possible effects retain unknown outcome. Local runtime dispatch futures are heap allocated to bound the caller's stack use.

Normal conversation opening keeps archived and closed sessions inactive. An unresolved submission blocks attachment; explicit resume returns an error until that original input is resolved. Attaching a live binding does not require the Harness to support restarting a closed session. Native resume follows the resources extension when installed and refreshes transcript and queue after attachment. Presentation drafts are not part of that execution transition.

After an acknowledged explicit shutdown, the lifecycle command waits for the execution owner lock to be released. It does not probe listeners during their teardown or repeat the shutdown mutation. Initial owner verification and exact-target shutdown guards remain required.

Backend tests exercise these application operations against a real runtime, SQLite stores and installed extensions. A failing provider applies a setting before losing its acknowledgment; recovery reads the resulting value and verifies exactly one update attempt. Tests also cover stale targets, invalid launch intent, missing directories, revision guards, worktree ownership and live worktree removal protection. Native IPC tests verify application dispatch, while view tests verify one application request, native queue rendering, recovered controls and draft retention.

## Public contracts

`packages/contracts` owns the canonical runtime JSON Schema and generated TypeScript definitions. The desktop application consumes this package directly over its native IPC integration; it does not depend on the TypeScript transport SDK. The SDK consumes the same contract package. Native application projection types and the structured SSH readiness result are maintained alongside generated runtime types.

Local packaging produces separate contracts and SDK archives. The staged SDK depends on the contracts version, without retaining a source-tree file dependency. Conformance checks install both archives in a disposable external consumer and verify declaration resolution and runtime imports.

## Remaining boundaries

The browser and native desktop share application workflow ownership. Views retain presentation reducers, drafts, navigation, form interaction, optimistic feedback and view caches. Rendering resource lifetimes and explicit read refreshes may still require frontend sequencing. Browser observation still uses HTTP polling; a persistent browser observation adapter remains follow-up work. Single runtime actions can pass through the gateway because authoritative validation and execution remain in the runtime.

Queue admission, steering and general extension mutations do not gain durable exactly-once guarantees from this separation. Authored metadata is recovery information, not proof of native acceptance. Cross-client draft synchronization and production provider recovery qualification remain separate work.

The executable composes separate `src/client/` and `src/runtime/` module groups. The first owns native connections, projections, startup and node administration; the second owns execution-owner lifecycle, runtime transports and service integration. Both remain in the same executable crate. A crate extraction should follow stable transport interfaces; forwarding-only crates would not establish useful ownership boundaries. Hosted accounts and cloud services remain outside the public application and runtime.

## Development qualification

The application crate passed its unit and integration suites on Linux and macOS, including the native workflow and managed IPC checks. The browser HTTP integration exercises authored input, duplicate-key recovery, exact-target rejection and canonical transcript projection against an authenticated runtime. Native history tests verify recovery after a fork, exclusion of rolled-back metadata and sequence remapping. Frontend tests, TypeScript validation, the production frontend build, native manifest validation and application Clippy checks passed. The system WebView host built and passed its native tests on macOS arm64.

A disposable native acceptance run exercised the actual frontend in the system WebView with test-only DOM input and result reporting added to an external asset snapshot. It opened a saved conversation, displayed node settings, submitted a mock turn through the native bridge, retained an authored draft, and closed the client while execution remained active. A new native window recovered the same runtime identity, draft and completed response. The canonical transcript contained the bootstrap input and exactly one subsequent input; the native client journal retained one submission. WebView reload repeated recovery without another prompt.

This is development qualification with a scripted Harness. It does not establish production provider access, OS pointer interaction, pixel-level acceptance, minimum supported OS versions, Developer ID signing or notarization. Qualification artifacts and disposable execution data remain outside the repositories.
