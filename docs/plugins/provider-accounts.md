# Provider accounts

`swarmie_providers::accounts::Accounts` owns account and model selection. It uses
injected account storage, credential custody, a protocol driver and a clock. It
has no concrete provider, native keyring, executor or storage backend dependency.

## Construct the account owner

Use `Accounts::new` with a checked `ProviderOwner`, the injected adapters, a clock,
the metadata cache capacity and explicit `RecoveryLimits`. The owner binds the
exact principal and stable profile UUID. Use one account owner for that scope.
Keep its native physical resources alive until started operations settle.

`AccountStore` loads a classified `StateLoad`. Its `save` method requires the
expected revision and one checked `ProviderState`. The state contains accounts,
immutable credential references and one optional workflow. Its decoder checks
owner, bounds, duplicate keys, identities, references and replacement images.
Private account configuration does not enter RPC account projections.

The storage backend must establish qualified physical settlement, visibility and
required durability before its observations can recognize a result. A byte read
alone cannot satisfy that contract. Unsupported guarantees produce an error.
`Absent` cannot initialize an empty state. `Provisioning` supplies plan data;
`initialize` still requires the backend's independent positive private receipt.
Legacy metadata remains gated for explicit private adoption. The common workflow
does not infer adoption authority from readable files or credentials.

`CredentialStore` provides pure `check_reference` and physical `stage`, `read` and
`retire` operations. Each operation uses an exact `CredentialReference`. A reference
binds the owner, account or operation slot, and immutable generation. It contains
no secret, path or access permission. Staging cannot overwrite a different sealed
value or an unowned partial generation. Retirement requires independent native
ownership and exclusion. There is no selected credential key or legacy fallback.
`CredentialRead` distinguishes Complete, Missing, Partial and Invalid. Unavailable
observations remain errors. Only Complete contains zeroizing secret material.

`AccountDriver` owns configuration, discovery, registration validation and remote
protocols. Its pure `protocol_binding` captures the exact injected protocol
configuration. Its pure `needs_renewal` validates the captured account and credential
with the supplied time. Renewal dispatch uses those same inputs and time. Recovery
does not replace lost time or submitted configuration with current defaults.

## Select a model

Call `Accounts::select` with a live Tembra context, a `SelectModel` request and a
trusted `ModelFactory`. The factory is not an RPC input. Every Active workflow
gates new selection, renewal and mutation, including authorization Waiting and
cleanup. Inspection and authorized private reconciliation remain available.
Already admitted physical work retains its existing lifecycle owner.

`ModelChoice::Default` uses the configured primary model, or the first available
model when no primary default exists. `Auxiliary` uses the auxiliary setting or
the primary selection. `Exact` requires the selected model. Invalid configured
models produce an error. Selection does not switch provider routes.

An absent reasoning override uses the applicable primary default. An explicit
empty override clears reasoning. Other values must match a supported setting.
A separate auxiliary model does not inherit the primary reasoning setting.

The common owner captures the account, validates selection and prepares one
immutable catalog snapshot. It reuses the existing bounded Catalog cache and
loads the embedded source once. It does not fetch metadata during selection or
inference. Required metadata precedes inference credential access. Explicit
credential-free captures do not probe custody. A credential capture uses only
its exact bound reference and validated registration.

Before a required refresh, Accounts checks reference shapes and persists intent
and ExchangeMayStart. It calls renew once. Lost exchange results remain Active;
a readable previous token does not prove that its refresh token was not consumed.
Verified replacement material must reach durable CandidateReady before account
publication. The returned model retains the same snapshot and existing physical
permission checks. This workflow adds no request engine, registry or scheduler.

## Settings and authorization

Accounts implements `ProviderSettings`. Listing uses bounded chunks and
revision-bound cursors. Availability describes custody presence or an explicit
credential-free account. It does not prove entitlement or model capabilities.
Configure records its intent before the driver runs. Candidate publication
preserves an associated account's ID, defaults and private extension fields.
Defaults use the same exact state-replacement discipline without custody.

`accounts::authorization::orb` exposes `poll`, `retry`, `complete` and `cancel`.
Tembra owns each session and closes its protocol resource. A session key is not
the internal operation UUID. Before begin_authorization, Accounts preflights an
Operation escrow and persists ExchangeMayStart. The browser protocol can exchange
before poll. Accounts seals and compares the exact issued secret, then persists
IssuedReady before GET-only discovery. It verifies association and the final
reference before staging an account generation. CandidateReady precedes publication.
No plaintext secret or credential digest enters the ordinary state file.

A Complete read at a planned reference below a verified-stage checkpoint is not
proof of the operation's successful stage after a restart. An unequal sealed
collision followed by refusal and a crash can produce that observation. If exact
original material or ownership proof is lost, the workflow remains Active. It
does not adopt, publish or retire the collision, or repeat an exchange. IssuedReady
requires Authorize, the matching Operation escrow, captured protocol binding and
the original account/reference image. Discovery can resume from that checkpoint
without another login. Pre-checkpoint material loss requires private disposition.

## Recognize store outcomes

`StoreWriteError::NotApplied` confirms that this mutation did not apply. Unknown
means that its result is not established. Observer cancellation is not rollback.
Native adapters retain their actual lease and permit until physical work ends.
No later effect follows an uncertain checkpoint in that invocation.

Accounts records exact predecessor and target revisions before publication. It
publishes the account and immutable reference together with Committed. A qualified
observation of that exact target and captured image can resume cleanup. An old or
higher revision does not authorize another uncertain publication. Protocol drift
keeps the workflow gated. Recovery never repeats an uncertain exchange or rebuilds
lost configuration from defaults.

Cleanup remains Active and retires only unbound references no longer needed by
the operation. The native adapter must establish staging ownership and settlement.
A missing descriptor alone is not erasure. Partial, invalid or unavailable data
cannot be silently cleared. After qualified absence of every required generation,
Accounts checkpoints the exact clear target before removing the workflow. An
uncertain clear is not repeated from its predecessor image.

The original live session retains one safe publication receipt. It contains no
credential and cannot override durable state. After exact recognized completion,
its next poll can publish Connected without another exchange. Cancellation still
prevents Connected. Another session cannot claim that receipt. If the resource is
closed, account listing exposes recognized state without recreating a session.

The common owner supports initialized, qualified injected stores and isolated
crash fixtures. It supplies no native initialization, adoption, keyring backend,
platform fence or protected erasure proof. Native construction and mounting need
those actual backend guarantees. Unsupported restore/quarantine and pre-verified
material loss remain explicit recovery gates.

See [Providers](providers.md) for final request construction.
See [Model catalog](model-catalog.md) for model facts and route qualification.

## Native file and I/O mechanics

`swarmie_providers::native::NativeAccountFile::new(path)` captures an unbound native
metadata file without I/O. Its typed methods below preserve private extensions
and use an explicitly supplied physical I/O owner. This mechanical helper does
not implement the recovery `AccountStore` port: visible bytes alone do not supply
qualified settlement, initialization or readiness. Native recovery construction
and mounting require the actual backend authority and platform evidence.

`NativeCredentialKeyspace` provides pure immutable-reference preflight. The host
injects the exact owner, existing namespace and explicit encoded reference,
username and protected-descriptor byte bounds. No platform defaults or custody
availability are inferred. `plan` checks the owner and bounds, then derives
versioned usernames from a length-prefixed binary tuple. Account identifiers
retain their exact UTF-8 bytes. The result covers one descriptor and 64 possible
part keys in the existing provider keyring service. Usernames are already fully
qualified and must not receive the legacy namespace prefix again.

Planning performs no I/O and grants no ownership, reservation or access. It can
refuse a known-unsavable escrow before authorization begins. Actual immutable
staging/read/retirement, protected legacy adoption, profile initialization and
native settlement remain separate requirements. The current node composition
does not use this planner or activate durable recovery.

`NativeProviderLease::acquire(lock_file)` acquires an actual exclusive file lock
without truncating existing contents or creating a profile directory/identity.
The path is trusted embedding configuration. `NativeProviderIo::new(lease,
executor)` retains that lease and an existing Tokio executor. All I/O handles
sharing the lease use its single serialization gate. `run` acquires the permit
before blocking dispatch and moves both permit and lease into the physical
closure, so observer loss cannot cancel dispatched work or release its ownership.
Recognition reads using that same owner wait until earlier physical work ends.

This is local file ownership and closure lifetime, not initialization evidence,
account authority or a fence for platform requests that survive process loss.
Callbacks must finish their owned synchronous work before returning and classify
their own publication result. Lost physical completion is `Unknown`. The host
retains its executor until started work settles. This component is not mounted
in the current node composition or used as an automatic recovery authorization.

`NativeAccountFile::load_provider_state` observes typed state through that shared
I/O owner with bounded reads and checked owner/codec validation. It distinguishes
an initialized image, exact legacy metadata and actual absence. Corruption,
oversized input, symbolic links and nonregular files are refused intact. It never
produces provisioning evidence from an absent file.

`replace_provider_state` checks the injected owner and exact predecessor revision
of an existing initialized image, encodes within bounds, stages and syncs a file
in the same directory, replaces the target, then syncs its directory. Unsupported
directory synchronization is refused before publication. Prepublication failure
is `NotApplied`; replacement or later durability uncertainty is `Unknown`. These
are cooperative revision checks under the supplied owner. The embedding must
qualify its stable lock/path binding and exclude untracked writers.
The active legacy provider workflow must not write that same path while these
methods are in use. The eventual runtime switch must replace that writer rather
than activate two writers for one account file.

These inherent methods do not change the live account ports or provide
initialize/adopt. A later successful read reports visible bytes; resolving an
earlier `Unknown` still requires actual settlement/durability evidence and does
not follow from an old revision or syntactically valid state alone. Current native
platform and recovery readiness remain separate qualification requirements.
