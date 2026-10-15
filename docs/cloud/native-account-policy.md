# Account admission and retained-data policy

The account service applies shared PostgreSQL admission counters to browser authentication and
native account routes. All replicas must use the same policy. Windows are fixed one-minute or
one-day periods measured by database time; a boundary can admit a burst from adjacent windows.
These are configurable starting budgets, not a production capacity assessment.

## Configuration

The optional `[native_accounts]` table accepts the following positive settings. Unknown fields,
zero budgets and out-of-range scheduling or retention values fail startup.

| Setting | Default | Scope |
| --- | ---: | --- |
| `requests_per_minute` | 600 | All admitted browser authentication and native account requests |
| `source_requests_per_minute` | 120 | Requests from one observed network source |
| `login_requests_per_minute` | 10 | New native handoffs and browser authentication ceremonies per source |
| `revocations_per_minute` | 120 | Separate service-wide budget for browser logout and native revocation |
| `refreshes_per_minute` | 8 | Successful rotations for one native credential family |
| `refreshes_per_day` | 192 | Successful rotations for one native credential family |
| `profile_writes_per_minute` | 20 | Name/image mutation attempts per authenticated account |
| `profile_writes_per_day` | 100 | Name/image mutation attempts per authenticated account |
| `avatar_writes_per_minute` | 4 | Image mutation attempts per authenticated account |
| `avatar_jobs` | 2 | Concurrent image decoders per service process |
| `receipt_retry_seconds` | 86400 | Profile mutation retry validity from its UUIDv7 issuance time |
| `cleanup_batch_size` | 256 | Maximum parent rows removed from each table per cleanup iteration |
| `cleanup_interval_seconds` | 60 | Interval between cleanup iterations |
| `trusted_proxy_addresses` | `[]` | Exact proxy peer IP addresses allowed to supply `X-Forwarded-For` |

For example, a profile may override selected values while retaining the other validated defaults:

```toml
[native_accounts]
requests_per_minute = 600
source_requests_per_minute = 120
profile_writes_per_day = 100
receipt_retry_seconds = 86400
cleanup_batch_size = 256
cleanup_interval_seconds = 60
trusted_proxy_addresses = []
```

A native installation identifier supplied by an anonymous caller does not establish a rate-limit
identity. Network admission precedes body parsing and identity-provider work. The global budget
is charged before per-source counters, bounding counter growth under arbitrary source addresses.
Native rotation budgets bind the credential family found from the validated token. Profile and
avatar budgets bind the authenticated account, and image limits apply before decoding. Repeated
or invalid admitted requests can consume network and profile-attempt budgets.

Revocation has its own global and source counters so exhaustion of other account-route budgets
does not prevent it. PostgreSQL failures reject admission as unavailable. Local native logout
still clears local authority and honestly reports unconfirmed remote revocation when necessary.

Only explicitly trusted socket peers may influence the source address through `X-Forwarded-For`.
The service walks the chain from the trusted right-hand suffix and stops at the first untrusted
address. Untrusted peers cannot replace their source through headers; ambiguous or malformed
chains from trusted peers are rejected. Configure the actual trusted proxy peers before using a
proxy deployment, otherwise clients share the proxy's observed source budget. A directly embedded router without socket metadata shares one anonymous source budget and never
trusts forwarded headers. Counters retain
hashed subjects rather than raw source addresses, browser cookies or account tokens.

## Rejection and retries

Rejected admission returns HTTP 429 with a positive `Retry-After` delay. Profile writes and
credential rotations have not occurred when their corresponding budget rejects the operation.
The native owner preserves a renewal credential after an explicit 429 and delays another attempt
until the advertised renewal cooldown. The client accepts delay-seconds (including zero) and
HTTP-date values, rounds future fractional waits upward, and caps supported waits at one day. Missing
or invalid values use a sixty-second fallback. It does not automatically replay mutations. A renewal
with an uncertain transport outcome retains the existing fail-closed behavior.

## Profile receipt validity

Profile and avatar mutation identifiers are UUIDv7 values. Their immutable issuance timestamp
bounds retry validity; a timestamp more than two minutes ahead of the service clock is rejected.
The desktop generates an identifier when creating the edit and retains it with an uncertain
request. The CLI generates one when `--request-id` is omitted. An explicitly supplied identifier
must satisfy the same UUIDv7 contract.

Within the retry window, identical requests return their original committed response. Reusing
an identifier with another body remains a conflict. Expiry is checked again after acquiring the
profile lock, using database time. Once an identifier expires, it is rejected even if its receipt
has been removed and the caller supplies a current profile revision. Expiry never turns an old
request into a new mutation. Clients must read confirmed state and explicitly create a new edit;
Refresh profile clears retained mutation receipts while preserving the authored name draft.

Changing retry policy can shorten the validity of previously issued identifiers. Keep policy
consistent across replicas and coordinate reductions with the promised retry window.

## Cleanup

The application owns a background cleanup worker. Each iteration deletes bounded batches of
expired sign-in attempts, expired native credential families, expired profile receipts and
expired admission counters. Expiry indexes support selection; row locks with `SKIP LOCKED`
allow concurrent workers to avoid rows held by active operations. Missed timer ticks are skipped.
Failures retain data for a later iteration and emit fixed diagnostics without proofs or identities.

Deleting an expired family cascades its renewal-token hashes. Used hashes for a family that has
not expired remain retained, including when the family is revoked. They support replay detection
and revocation using previously issued material. Cleanup does not delete users, app-owned profiles,
projects or browser-session data. Family batches can cascade multiple token-hash rows.

The additive migration preserves existing migration checksums. Pre-existing receipt records
receive the maximum supported seven-day storage-retention boundary; new receipts store their
configured UUIDv7-derived expiry. Storage retention does not provide legacy UUIDv4 API compatibility:
all profile writes and retries require UUIDv7 before receipt lookup. Earlier clients must be upgraded
and create fresh UUIDv7 intents; old UUIDv4 responses are not replayed. This follows the repository
policy that earlier development clients and persisted formats require no compatibility shim.
Applying this migration to an existing deployment is an explicit operational step and is not part
of isolated development qualification.

## Qualification and remaining work

Tests cover concurrent replica admission, bounded source-counter growth, reserved revocation
admission, cleanup batches and locks, retention of active token-reuse evidence, expired receipt
rejection, proxy spoofing, HTTP 429 before renewal consumption, and native vault preservation and
cooldown. Browser authentication, passkey and native profile contract tests run against disposable
PostgreSQL. The actual configured service and native CLI are also qualified using a fresh database
and the seeded memory identity.

The subsequent [session delivery](native-account-sessions.md) adds account-owned native session
management and service-authorized disablement. Production monitoring, deployment, WorkOS browser
acceptance and production capacity tuning remain separate qualification work.
