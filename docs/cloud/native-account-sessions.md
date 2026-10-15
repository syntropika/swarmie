# Native sessions and account administration

Native sessions belong to the stable Swarmie account resolved from trusted identity authority.
An installation UUID is caller-supplied metadata, not a trusted device identity. Workspace owners
cannot administer global accounts. Browser sessions remain independent of native session families.

## Account-owned session operations

| Endpoint | Result |
| --- | --- |
| `GET /api/native/sessions?after=<uuid>` | Current family, up to fifty other active families, and an optional next cursor |
| `DELETE /api/native/sessions/<uuid>` | Revokes the selected owned family and reports whether it was the requesting family |
| `DELETE /api/native/sessions` | Revokes all native families, including the requesting family and approved pending native codes |

Every endpoint requires a currently valid native bearer and rejects browser cookies and Origin
headers. Provider identity and any authenticating passkey are revalidated. Mutations then lock the
account and recheck the requesting family inside the transaction. An already revoked actor cannot
perform another mutation. A foreign or unknown target UUID is an indistinguishable successful no-op;
it cannot affect another account or disclose whether that account's session exists.

Metadata contains family and installation UUIDs, original authentication time, family creation time,
last activity and absolute expiry. It contains no token hashes, bearer values, provider subjects or
upstream session references. Lists exclude revoked and expired families. The current family appears
separately on every page; other families use stable UUID ordering and a fifty-row page boundary.
Concurrent additions or revocations can change subsequent pages. Refresh obtains a new first page.

Last activity updates on renewal and at most once per five minutes during native authentication.
The family retains its thirty-day absolute expiry regardless of activity. Used renewal hashes remain
available until family expiry, including after revocation. Native session deletion uses the reserved
revocation admission budget. Local installation logout retains its existing offline behavior.

Global native revocation advances the account's native authorization epoch. Codes approved under an
earlier epoch cannot create a family later, including when redemption was waiting on provider I/O.
Account locks precede family or approved-request locks. Provider synchronization occurs before the
transaction, avoiding an extra pool acquisition while account authority is locked. Renewal rechecks
family state after provider I/O; spent-token reuse still revokes the family during a provider outage.
A browser that remains authenticated can approve a new native login after global native revocation.

## Service-authorized administration

Account disablement is available through an explicitly configured service maintenance command:

```sh
server --config <service-config.toml> disable-account <account-uuid> --actor <operator> --reason <reason>
server --config <service-config.toml> enable-account <account-uuid> --actor <operator> --reason <reason>
```

The command requires the configured service database credentials. There is no browser or native
administration endpoint and no implicit workspace-role authority. The operator label attributes the
action; it does not authenticate the operator. Restrict configuration access and command execution
through the service operator's existing infrastructure controls. Both operator and reason must be
nonempty, contain no control characters and remain within 512 bytes.

Each operation atomically updates account status, advances the native authorization epoch, removes
local browser sessions and approved native requests, revokes native families, and records the
operator, reason, requested status and database time in `account_status_changes`. It never changes
the upstream identity provider's account or deletes users, profiles, memberships, workspaces, local
projects or execution nodes. Repeating either command invalidates any intervening local sessions.

Disabled accounts cannot synchronize authenticated identity, establish local browser sessions,
redeem native codes, renew families or authenticate native requests. Browser and native authentication
recheck local session authority after provider I/O. Profile writes also check active account status
inside their transaction. Operations already authorized and committed before disablement are not
rolled back. Re-enabling leaves prior cookies, tokens and codes invalid: fresh authentication is
required to establish new authority for the same stable user.

The additive migration preserves earlier migration checksums. Applying it invalidates pre-existing
approved native requests whose approval lacked an epoch. Existing native families remain valid and
receive metadata timestamps when the new columns are added. Applying the migration to a deployed
database is a separate operational action.

## Qualification

Disposable PostgreSQL tests exercise cross-account isolation, repeated targeted revocation,
pagination, mixed browser/native authority, native global revocation, retained browser authority,
disable/re-enable with retained profiles and workspace membership, and provider-I/O races involving
renewal, redemption and browser reads. Native-owner tests cover stale account intent, sanitized
metadata, credential invalidation and unconfirmed revocation without automatic replay.

Actual service and CLI acceptance used two independent native owners, a fresh database, the seeded
memory identity and protected credential storage. It confirmed individual and global revocation,
owner restart, administrative disable/enable, old browser-cookie rejection, fresh authentication
resolving the same account, and preservation of local content. The desktop settings surface was
checked at normal and compact desktop widths with synthetic session metadata.

This delivery does not deploy the service or change an existing account. WorkOS browser ceremonies,
Windows qualification, production monitoring, backup/restore validation and capacity tuning remain
separate work. Upstream provider administration, account deletion and a browser session-management
portal are outside this native-session capability.
