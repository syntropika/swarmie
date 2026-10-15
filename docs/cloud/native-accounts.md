# Native account grants and profiles

The account service provides a first-party handoff between a browser authentication ceremony and a
persistent native credential owner. Native families do not borrow the initiating browser session ID.
Browser logout therefore does not revoke an independent native family. Native authority revalidates
the configured provider subject and, when applicable, its authenticating passkey. A missing identity,
revoked passkey, disabled account or revoked native family cannot authenticate. Account-owned native
session revocation and service-authorized disablement are defined in [session policy](native-account-sessions.md).

## Endpoints

| Endpoint | Proof and result |
| --- | --- |
| `POST /auth/native/request` | Native installation UUID, exact loopback callback, state and S256 challenge; creates a ten-minute attempt |
| `GET /auth/native/authorize` | Binds the request to the browser and starts fresh hosted authentication |
| `GET /auth/native/approve` | Displays the authenticated email and requests explicit consent |
| `POST /auth/native/approve` | Browser cookie, approved Origin and CSRF token; approves the request once and returns a callback URL |
| `POST /auth/native/redeem` | One-use code, verifier, installation and callback; atomically creates an independent native family |
| `POST /auth/native/refresh` | Rotating native renewal credential; issues new access and renewal credentials |
| `POST /auth/native/revoke` | Current or previously issued renewal material; revokes that family without contacting the provider |
| `GET /api/native/profile` | Native bearer; revalidates current identity and returns app-owned profile fields |
| `PATCH /api/native/profile` | Native bearer, displayed account ID, expected revision and request ID; updates the name |
| `POST /api/native/profile/avatar` | The same profile proofs plus bounded image bytes or explicit null removal |
| `GET /api/native/sessions` | Native bearer; current family and bounded pagination of other active families |
| `DELETE /api/native/sessions/<uuid>` | Native bearer; revokes only a family belonging to the authenticated account |
| `DELETE /api/native/sessions` | Native bearer; revokes all native families and approved pending native codes |

Machine exchanges reject browser Origin/cookies. Native profile endpoints accept native bearer
credentials and reject mixed browser credentials. Existing browser-only credential and Workspace
routes retain their existing authentication/CSRF rules; native grants are not silently admitted to them.
All account responses disable caching. Telemetry records route templates, never query proofs or token
bodies. The approval page uses text rendering for account data and does not automatically approve GETs.

## Persistence

The native-account schema stores token hashes, explicit provider subject/credential references,
installation identity, absolute expiry and revocation state. It does not store raw native bearer
values. Access expires after fifteen minutes. Native families have a thirty-day absolute lifetime;
renewal never extends it. A reused renewal token revokes its entire family. Row locking makes
concurrent redemption/rotation atomic. The native owner must serialize requests rather than race
several clients against a rotating family.

`account_profiles` contains app-owned names, normalized avatar content and optimistic revisions.
Provider synchronization cannot overwrite these values. Removing an image explicitly suppresses a
provider default. Profile receipts reject request-ID reuse with different content and return the
original committed response for an identical repeated write. Account IDs prevent cross-principal
writes from stale clients. Avatar normalization accepts PNG/JPEG/WebP within bounded byte, dimension
and decode-allocation limits; stored JPEG thumbnails omit source metadata.

The migration targets fresh development schemas. This change does not deploy the service, reset
existing data or reconcile a historical deployment ledger. Native grants for hosted resources,
and the broader account portal remain separate production work. Service-authorized administration does not
deploy or reconcile an existing installation. The current entry path reuses configured hosted sign-in;
credential-only issuers need a dedicated portal before real browser-native onboarding is available.

## Admission and retention

[Account policy](native-account-policy.md) defines configurable shared PostgreSQL budgets,
reserved revocation admission, bounded image concurrency and periodic expired-data cleanup.
Profile edits use time-bound UUIDv7 identifiers: expired requests cannot become new mutations
when receipt records are deleted. Active-family renewal hashes remain available for reuse detection.
This capability is qualified in isolated environments; it does not deploy a hosted account service.
