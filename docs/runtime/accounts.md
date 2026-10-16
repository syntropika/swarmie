# Native accounts

## Ownership

Accounts are optional. Local Projects, chat files, Harness authentication and SSH nodes do not
require a Swarmie account. Signing in does not publish files or enroll execution nodes.

The `swarmie-accounts` library owns account state, pending PKCE requests, renewal serialization,
profile access and vault interfaces. The `swarmie account serve` host is a persistent process
independent of execution runtimes and desktop gateways. A desktop window and CLI clients attach to
one owner. Closing a client neither stops the owner nor cancels a pending login.

The native gateway forwards `account/*` requests to this local per-user owner before node routing.
No selected-node readiness check or Harness startup is required. The owner exposes only sanitized
snapshots and profile results to presentation clients. Human account tokens are never forwarded to
remote execution nodes. Node enrollment and scoped machine grants remain separate hosted capabilities.

## Storage and lifecycle

The owner's `identity/` directory beneath the application data root contains an installation UUID,
configured account origin, process locks, local IPC endpoint and non-secret vault invalidation marker.
Unix directory/file modes and a protected user-only Windows DACL protect local IPC credentials.
Account refresh tokens are held in Secret Service on Linux, Keychain on macOS or Credential Manager
on Windows. There is no plaintext account-token file fallback. Access tokens and profiles are held
in native memory; only the origin and renewal credential go into the secure vault.

A locked or unavailable vault has an explicit unavailable state. Session-only sign-in is a deliberate
choice and lasts until the owner stops. A non-secret marker invalidates retained secure credentials
on local logout even if physical vault deletion must wait for the vault to become available. Failure
to write that marker is an error; the owner does not claim a successful logout.

An OS lock prevents concurrent owners for a data root. All renewal, redemption, profile mutations
and logout operations serialize within that owner. Accepted profile writes also carry the displayed
account ID, expected revision and request receipt ID. A stale view cannot edit another signed-in account.

An active owner renews expiring access credentials for recently used accounts, including after a
window closes. Unused accounts stop periodic renewal after the activity window. The next authenticated
read or mutation checks credentials again. On restart, the owner restores only the renewal credential
and obtains fresh access and identity before account-dependent work. Pending logins do not survive
an owner restart and must be started again. A renewal with an uncertain outcome is not replayed.

Local logout clears native authority first and attempts hosted revocation afterward. The result
reports whether remote revocation was confirmed. An unavailable Cloud service does not stop local
projects or execution. Browser logout and native logout have independent grant lifetimes.

## Initial sign-in and profile

Configure the hosted account origin through Account settings or:

```sh
swarmie account configure https://accounts.example.com
swarmie account login
swarmie account read
```

Only HTTPS origins and explicit loopback HTTP development origins are accepted. This implementation
does not assume an already deployed account service or alter an existing database/deployment.

The owner binds its callback listener before requesting browser authorization. The hosted service
performs browser authentication and explicit same-origin consent, then returns a short-lived one-use
code to the exact loopback callback. The owner validates state and redeems it with the retained PKCE
verifier. The browser cookie and upstream provider credentials never become native-client credentials.

The account control at the bottom of the activity rail shows the signed-in avatar and identity,
with a user-icon fallback when no image is available or an image fails. Its signed-in menu offers
Profile, Settings, Account security and Sign out. Signed-out and unavailable states expose sign-in
or recovery, and Settings remains available without an account. Profile and Account security open
sections of the full Settings workspace. Account security describes this installation's vault or
owner-session storage and scoped sign-out, lists active native sessions, and supports individual or
global native revocation with explicit confirmation. The browser retains its independent session.

The Account settings page supports sign-in, explicit cancellation, vault/session-only persistence,
read-only verified email, editable display name, image upload/removal, profile refresh and logout.
Avatar input is bounded and decoded on Cloud, resized and re-encoded to JPEG to discard source
metadata. Image limits are 1 MiB and 2048 pixels per side. App-owned profile values are independent of
provider synchronization. Names and image changes use optimistic revisions and idempotency receipts;
explicit retries preserve their receipt after an uncertain response.

The owner retains the authorized browser URL in the sanitized pending-login snapshot. Closing and
reopening Settings can therefore reopen the same browser attempt until completion, cancellation or
expiry. Closing a view does not cancel the attempt. Owner restart ends a memory-only pending login.

An edited display-name draft retains the profile revision at which editing began. Updates from
another window neither overwrite that draft nor implicitly rebase its write. A conflicting save
remains visible. Explicit Refresh profile refreshes the confirmed account state and rebases the
retained draft to that revision; changing the signed-in account clears drafts and mutation receipts.

CLI commands also include `cancel`, `logout`, `profile`, `avatar` and `stop`. Use `--no-open` to obtain
the authorization link without invoking a browser and `--session-only` for explicit ephemeral use.
The browser must reach the owner's loopback callback. Device-code login is not implemented yet.

## Public native contract

`packages/contracts/account.d.ts` describes account operations separately from execution RPC.
Consumers use the client gateway's authenticated native bridge; web presentations cannot read account
credentials. Supported operations are `account/read`, `account/observe`, `account/configure`, `account/login/start`,
`account/login/cancel`, `account/logout`, `account/sessions/read`, `account/sessions/revoke`,
`account/sessions/revoke-all`, `account/profile/read`, `account/profile/update` and
`account/profile/avatar/update`. Null image content removes the current image. Owner inspection and
shutdown are private lifecycle operations and are not forwarded from the desktop renderer.

`account/observe` accepts an optional cursor containing `ownerId` and `revision`, and returns a
cursor with the latest sanitized account snapshot. A matching cursor holds the request until a
change or a 25-second timeout; a missing or different cursor returns the current snapshot immediately.
Each client retains its own cursor, so one reader cannot consume another reader's update. The owner
publishes changes after commands, browser callbacks and its background lifecycle tick. Observation
reads retained state only: it performs no Cloud profile read or credential renewal.

The rail, profile and security views share one AccountStore per window, independent of execution-node
selection. Initial attachment and reconnection use an account read to validate state, followed by
observation. The owner incarnation in the cursor reconciles restart, and older publications cannot
overwrite newer state. Reconnection never replays mutations. Account and observation failures remain
visible while local work stays available.

Saved-account switching, device-code login and account-linked remote enrollment remain follow-up
capabilities. Service-authorized account disablement is implemented independently of native execution;
deployment lifecycle remains separate work.

## Native session management

Account security shows the requesting installation separately from other active native sessions,
with sign-in, last-activity and absolute-expiry timestamps. Activity can lag by five minutes.
Installation UUIDs are declared metadata rather than trusted device identity. Session listing is
explicit, paginated and separate from account observation; it never exposes token hashes or provider
references. Refresh starts a new page sequence, and Load more sessions appends bounded pages.

Individual revocation affects only the chosen family belonging to the displayed account. Global
native revocation includes this installation and approved pending codes. The desktop requires an
explicit confirmation, initially focuses Cancel, and explains that local files and execution nodes
remain available. The browser session remains independent. Revocation errors appear beside the
corresponding controls, with focused recovery through Refresh session state. An uncertain mutation
is never automatically replayed or described as confirmed success.

```sh
swarmie account sessions
swarmie account sessions --after <next-cursor>
swarmie account revoke-session <session-uuid>
swarmie account logout-all
```

The CLI reads the current confirmed account before constructing a session intent. Native RPC methods
`account/sessions/read`, `account/sessions/revoke` and `account/sessions/revoke-all` require
`expectedAccountId`; read accepts an optional UUID `after` and targeted revocation requires
`sessionId`. The owner serializes these operations with its credentials and rejects a stale account
intent before contacting the service. Listing returns `current`, `others` and `nextCursor`, with
ISO 8601 metadata timestamps. Revocation returns a sanitized account snapshot. Confirmed revocation
of the requesting family clears protected credentials and publishes account invalidation to clients.
Service rejection of current native authority also clears restorable credentials.

Service-authorized disablement blocks native and browser authority without deleting account content
or local resources. Re-enabling does not restore previous cookies, grants or codes; a new sign-in
establishes authority for the same stable account. Administration belongs to the private service and
is not exposed through the desktop or execution nodes. See the hosted
[session policy](../cloud/native-account-sessions.md) for the operator workflow.

The session delivery passed 175 frontend tests, twelve account-core lifecycle tests, owner integration,
thirty-nine PostgreSQL browser/native/passkey/limit tests, and actual service/CLI acceptance using two
independent native owners. Desktop and compact settings captures qualify the rendered session list,
confirmation and local failure recovery. This evidence uses isolated fixtures rather than a deployed
service or a real WorkOS browser ceremony.

## Qualification and limits

The scoped account extension passed 170 frontend tests, eight core lifecycle tests and owner
integration tests. Native app unit tests passed 44 cases, with one unrelated platform-specific case
ignored. A real Cloud HTTP router with fresh disposable PostgreSQL and a seeded synthetic memory
identity qualified native login, profile changes, concurrent observation, protected persistence,
owner restart requiring fresh credential renewal, revocation and session-only login lifetime on Linux
and macOS. Linux Secret Service and a macOS GUI Keychain fixture passed. A native macOS WebView
exercised the menu, profile, security, event-driven completion, window close/reopen, secure restore
after owner restart and renderer sign-out.

Additional real-service acceptance confirmed refresh-token rotation in PostgreSQL, service-side
revocation clearing native authority and publishing invalidation, and an unavailable service
retaining the protected login. Offline logout reported unconfirmed remote revocation, and local
invalidation prevented restoration after owner restart.

Vault lock/unlock recovery is covered by a controlled mocked-vault test; an actual operating-system
vault lock was not induced. Windows qualification and the WorkOS hosted browser ceremony remain
unqualified for this extension. The synthetic identity issuer has no hosted OAuth page: the disposable
qualification used the real password API and explicit approval API. Deployed-service acceptance remains open. This work performed no
deployment, changes to an existing database or hosted enrollment.

## Admission responses and expiring receipts

The account service returns HTTP 429 with Retry-After when an admission budget is exhausted.
The native owner preserves the current protected renewal credential after that explicit rejection
and defers another renewal attempt until the delay elapses. Delay-seconds (including zero) and
HTTP-date values are accepted; waits round upward to avoid early retries and are capped at one day.
Missing or malformed values use sixty seconds. Unknown renewal outcomes are still
not replayed. Profile mutations remain explicit user actions.

Profile and avatar request IDs are UUIDv7 values whose issuance time bounds the server's retry
window. The desktop creates and retains an ID with each edit; the CLI creates one if --request-id
is omitted. Expired IDs are rejected even after receipt cleanup, so old writes cannot execute again.
Refresh profile reads confirmed state and clears retained name/image receipts before a new explicit
edit. See [account policy](../cloud/native-account-policy.md) for configuration and retention rules.
