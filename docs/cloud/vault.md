# Hosted encrypted vault contracts

The independent hosted implementation authenticates native account families, orders signed
commands, stores encrypted records and wraps device-targeted HPKE envelopes. It has no vault data
key and cannot decrypt secret values. Client/runtime builds do not depend on hosted source packages,
documentation files or hosted credentials.

## Admission and recognition

Vault routes require actual native bearer authentication. Browser Cookie or Origin context is
refused. Authentication runs before JSON extraction and applies only to these routes. Account,
current family and bounded complete active-family facts come from the authentication owner, never
caller installation labels. The HTTP request budget is 512 KiB.

The POST endpoints are `/api/vault/commands`, `/api/vault/catalog`,
`/api/vault/recognize/{operation}`, `/api/vault/fence`, `/api/vault/fence-found`,
`/api/vault/records`, `/api/vault/authority`, `/api/vault/check`, `/api/vault/tickets` and
`/api/vault/redeem`. Wire values are independently versioned in client and hosted vault modules;
an independently generated signature fixture checks canonical record-command agreement.

Commands sign the JCS tuple `("vault-command/v1", operation, nonce, device, action)` with Ed25519.
A record operation equals its immutable revision. Recognition compares the exact payload digest.
A missing receipt means Unknown. Atomic fencing records NotDispatched or returns the existing exact
receipt. Founding and lifecycle fences prove possession of their original signing key and bind
an exact payload digest. A lifecycle fence never needs to retain or retransmit its inner envelope;
another signer cannot poison the operation's recognition.

## Trust and keys

Founding admits one mutable personal root through an expected-current comparison. Old roots become
readable archives with certificates and encrypted envelopes retained. Replacement does not silently
mark an archive exposed. Restoration requires surviving old-root EnrollDevices authority and the
exact current root. Restoring an exposed archive retains its exposure and refuses value creates
and replacements; the native UI warns before confirmation. A new empty vault cannot recover values
whose private enrollment keys were lost.

Enrollment binds account/root/epoch, recipient, both public keys, roles, approver, nonce, exact
inner-envelope digest and vault-key commitment. Admission checks the existing chain, role subset,
unique keys and IDs. Chain depth is limited to eight; a root has at most 128 enrollments. Login
does not enroll a device. Both recipient and approver independently compare the full proposal
digest. A new native family proves possession of its admitted signing key to rebind.

Retirement differs from compromise. Compromise names the exact affected subtree and current
authority sequence, removes that subtree and preserves an exposure warning. The server recomputes
the set and refuses removing the last complete enrollment authority. Account-session revocation
remains owned by authentication.

Device-encrypted inner envelopes receive an outer versioned XChaCha20-Poly1305 wrapper. Associated
data binds root, recipient and wrapping-key version. Operators must retain and qualify restoration
of every needed wrapping-key version. Rewrapping neither rotates credentials nor gives Cloud a
vault data key. Missing or unavailable custody has no plaintext fallback.

Management transport is mutually pinned TLS 1.3 with raw Ed25519 public keys and a domain-separated
exporter. The manager signs `("vault-client-ticket/v1", intent, manager_family)`; its ticket binds
both endpoints, root, shell generation, ephemeral resume key, exporter, challenge and nonce. The
node signs `("vault-redeem/v1", ticket, intent, node_family)`. Both native families must be active.
Tickets are one-use with a 60-second authentication deadline, not a vault inactivity expiry. Native
admission confirmation and resume proofs bind the actual new mutual channel and fence old
attachments. Public IDs cannot reconstruct these native capabilities.

## Persistence and finite budgets

The repository compare-and-swaps one account document. Accepted and definitively refused commands
retain exact immutable receipts. Record admission checks expected predecessor, monotonic successor
and unique live name. A stale concurrent writer cannot overwrite the winning version. Offline
peers are not part of an acknowledgement barrier; they fetch confirmed records using an explicit
hosted cursor. A page contains at most 128 records and 4 MiB of serialized records.

The initial document adapter has finite budgets: 32 roots, 8,192 value-history records per root,
and up to 16,384 records including additional deletion tombstones. Routine operations stop at
24,576 receipts/events or a 56 MiB account document. Security maintenance and digest fences have
a separate reserve of 8,192 entries/events and 8 MiB, up to an absolute 32,768 entries/events and
64 MiB document. Thus routine exhaustion does not consume the reserve needed for removal, rebind
and archive restoration. These are finite reserves, not unlimited availability under repeated
maintenance. Exhaustion refuses further growth instead of deleting trust history or receipts.
The adapter loads the complete account document and requires capacity planning before deployment.

## Qualification

Hosted tests use the production router, real native PKCE middleware, distinct manager/node families,
signed enrollment/rebind, one-use tickets, exact fencing and PostgreSQL persistence. Primitive HPKE,
private entry and keyring qualification belong to the native client. A separate composed harness uses real HTTPS, this router and PostgreSQL with the actual native
Cloud adapter, distinct enrolled endpoint keys/families, TLS exporters, release, rotation and
ephemeral resume. Its local journal is an in-memory fixture; SQLite is qualified separately.
Synthetic wrapping-key backup/restore tests preserve both archive/current generations and rewrap
the exact inner bytes. These checks do not qualify a deployed operator's backup procedure or
a complete two-application desktop enrollment flow. See [Node vault](../client-runtime/vault.md) for native behavior
and the limits of capture, masking, offline use and final model protection.
