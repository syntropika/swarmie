# Node vault and private secret dispatch

The node owns encrypted secret records, approvals, capture journals and execution-time decryption.
Cloud authenticates account sessions and orders encrypted mutations. Cloud never receives the vault
data key or a secret value. A new device needs approval and independently compared trust material
from an existing enrolled device; account login alone does not establish that trust.

## Modules and composition

`swarmie-permissions` issues native human-owner and logical-execution capabilities. Serializable
IDs do not grant authority. A retained logical agent has one generation across conversation turns;
each physical Tembra context receives a narrower scope. Cancelling that context or dropping its
owner fences pending approvals. Children receive distinct identities and cannot inherit a main
agent's conversation approval. Core grants come from compiled distribution choices, have visible
permissions, and require no acceptance. Other grants require a real, exact human decision; provider
identity and permission generation are checked again at use.

`swarmie-vault` is independently usable with supplied storage, account admission and execution
owners. Optional features add native keyring custody, HTTPS Cloud access, HTTP/Bash executors and
reference-only Tools declarations. Tembra owns service resolution, references, State and lifetime;
these modules do not introduce another service container or transport engine.

`swarmie-node::NativeNode` composes protected native custody and the private UI Orb.
The embedding creates a main or child `NativeAgent`, retains it for its logical lifetime, and uses
`enter` for each actual physical context. It can install the agent's model Orb and Vault Tools into
its existing host. Every connection in that model Orb shares the node's final submission guard.
`tools::definitions` supplies the five reference-only tool definitions for the existing Tools
catalog. After sign-in, unlock or vault selection, `NativeAgent::refresh` consumes the old logical
owner and returns a new generation with the current vault attachment. The embedding must replace
its model/tool Orbs before the next turn; old pending calls and approvals remain fenced.
Merely installing the private UI Orb does not install an Agent Loop or a model provider.

The desktop WebView communicates with the private UI through an origin-restricted Native SDK byte
carrier and ordinary Tembra RPC. Release builds accept the bundled application origin. Development
origins are compiled only into debug builds. Raw Native SDK session recording/replay disables this
private carrier because those journals are not qualified to redact private entries. Queue pressure
ends an attachment explicitly. Attachment generations prevent late old frames from entering a new
peer. The browser reattaches and reacquires the read reference after channel failure; it never
repeats failed mutation calls. Replacing the WebView attachment does not itself lock the retained
native vault owner.

## Storage and unlock

The account session has its own OS keyring entry. REST authentication and account requests use
the configured API origin; the separate authentication endpoint pins only the allowed public
browser authorization origin/path and never receives a bearer token. Superseded handoff grants
are best-effort revoked without installing them in protected storage. Separate protected entries contain device private
keys and independently pinned signed enrollment material. Provisional enrollment keys cannot unlock
a vault. No password fallback, saved data key, inner envelope cache or saved-value reveal method is
provided. The management UI can replace or delete a value.

A random 256-bit vault key exists only in native memory after fresh hosted admission. Each record
uses independently derived encryption/authentication keys and XChaCha20-Poly1305 with a random
192-bit nonce. The immutable account/root/epoch, secret identity, revision, predecessor, successor,
policy and record kind are authenticated. Device envelopes use HPKE Base mode with X25519,
HKDF-SHA256 and ChaCha20-Poly1305. Enrollment chains use Ed25519 signatures and exact pinned digests.

SQLite stores ciphertext records, signed immutable pending commands, rejected encrypted drafts,
and an Ed25519-authenticated local capture/recovery snapshot. Local snapshot replacement is a
compare-and-swap transaction. An uncertain storage owner gates new leases. Snapshot signatures
reject modification; they do not provide a hardware monotonic counter or prevent restoring an
older complete signed backup. SQLite qualification covers process crashes and independent
connections, not a guarantee against power loss on every filesystem.

There is no inactivity, sleep or heartbeat unlock-expiry timer. Last-confirmed values can be used
while disconnected until application/runtime exit or observed authority reduction. A disconnected
remote node cannot immediately observe application closure; this accepted limit ends when closure
or revocation becomes observable, or the node restarts. Authentication challenges, network calls and
physical producer settlement have bounded deadlines; those are not vault inactivity timers.

## Scoped use and replacement

A secret's project restriction and stable configured agent restriction both have to match. A
credential lease also requires the actual native execution, current plugin grant, exact provider,
operation/physical attempt, destination and current policy generation. Approval is either per
operation or for the main logical agent in that conversation. A child never receives the latter.

An executor prepares references first, obtains any human approvals, synchronizes connected confirmed
state, and resolves the newest confirmed value immediately before dispatch. Updating a credential
does not mutate a request already physically dispatched. A confirmed replacement awaiting ordered
local reconciliation prevents leasing the superseded cached value.

The available tools are `vault_list`, `vault_request_input`, `vault_capture`, `vault_http` and
`vault_bash`. They return metadata, references or sanitized receipts. Private input uses the chat
card and a separate native shell port; it never becomes an ordinary conversation message. A
compiled `PrivateEntryHook` may receive a newly entered value only with its pinned current
`AcceptAndStore` grant. The embedding reads such opaque contributions from its active plugin index.
This hook does not grant existing-secret reads or decision authority. Its synchronous side effects
are not rolled back if another recipient or the subsequent core save fails.

HTTP injects credentials only as declared header data or at an exact JSON reference leaf. It uses
HTTPS, refuses redirects and automatic retries, and never expands credentials into URLs. Bash
receives credentials through declared `SWARMIE_SECRET_*` environment slots; source text is unchanged.
Exact same-call reflections are masked before publishing HTTP responses or stdout/stderr. JSON
strings and keys are decoded before matching. Truncated or unsafe escaped HTTP output can be
withheld. Arbitrarily encoded, derived or cross-call exfiltration is not covered. Arbitrary Bash
commands are not a credential sandbox.

## Blind capture and recovery

A capture freezes one target, expected revision, policy, actual execution, source profile and
settlement bound. Human permission to save a newly produced credential precedes issuance and is
separate from approval to use input credentials. Durable `Admitted` and `Dispatched` journal writes
precede physical source execution. The producer receives a one-value, one-target sealer rather than
a general vault key or read capability.

Supported profiles are native random generation, one HTTPS JSON string leaf and one Bash stdout
JSON string leaf. Bash capture publishes no residual stdout or stderr, including encoded copies.
Normal completion waits for physical settlement and durable encrypted candidate delivery. Logical
cancellation cannot discard a source that already owns its restricted settlement sink. A persistence
failure retains only the encrypted candidate for retry. Proven no-start and uncertain issuance are
different outcomes. Uncertain issuance is never automatically repeated.

An immutable original store command is recognized or atomically fenced before a held value can be
rebased. A human can then save that same privately retained value against a new expected revision,
or discard it. Discarding storage does not revoke a credential at its issuer. Definitive ordinary
save rejection similarly retains an encrypted draft; an uncertain save is never treated as absent.

New-empty founding preserves old enrolled devices' archive access. It is not recovery of old
secrets, automatic exposure classification, or automatic enrollment into the new root. Restoring
an archive requires retained old-root enrollment authority and an exact current-root comparison.
If all enrolled private keys are lost, old values cannot be recovered.

## Final model submission protection

`swarmie_providers::protection` is the Providers content-inspection module. It is enabled without a
permission prompt. Its pinned offline detector inspects final provider-shaped text/JSON, including
tool descriptions, schemas and decoded JSON fields/keys, before credential acquisition. Duplicate
JSON keys, unsupported coverage, truncated scans and unavailable enabled engines block submission.
Each model response and progress snapshot carries a safe submission-protection label: Pending,
Unprotected, Clean, HumanOverride or CannotInspectOverride. Disabled or absent protection never
produces a Clean label. Credential masking changes provider-controlled content, not this local
admission evidence or fixed protocol fields; reflected tool identifiers withhold tool calls.

Limits include 2 MiB of final content, 32,768 decoded fields and 128 findings. There is no online
credential validation or agent-controlled suppression mechanism.

The provider owns immutable final bytes. The admission handle owns those same prepared bytes and
binds them to the destination, operation, physical attempt, actual execution and policy/permission
generations. Credentials are acquired afterward and cannot swap the admitted body. A real human
can authorize that exact blocked attempt once. Disabling this guard produces an explicitly
Unprotected node setting; it does not silently substitute an unavailable enabled detector.

## Qualification and limits

Behavior tests cover encrypted records, scopes and child isolation, uncertain synchronization,
private HTTP/Bash injection and masking, capture settlement, primitive known answers, protected
custody, final provider bytes and native byte RPC. Cloud has separate production-router/PostgreSQL
qualification. Browser fixtures exercise the actual client screens with a synthetic private backend;
those fixtures do not establish cryptographic or hosted admission correctness.

A composed harness now exercises the actual native Cloud adapter over real HTTPS and the production
Cloud router/PostgreSQL with distinct native families, independently compared bootstrap keys,
mutually pinned TLS exporters, HPKE release, replacement and proof-based resume. Its local journal
is a fixture; SQLite reopen/CAS is qualified independently. Latest native Rust suites pass on Linux
and macOS, with separate actual GUI keyring evidence. Production browser bootstrap runs under a
build-generated CSP with Trusted Types and no child frames. Full two-application enrollment, every
target desktop WebView, and the deployed operator's wrapping-key restore still need qualification
before real credentials. No malware, privileged same-process extension or arbitrary
child-process isolation guarantee is made. Organization/workspace credentials, biometric unlock,
saved-value reveal and Cloud recovery are outside this implementation.
