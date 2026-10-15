# Hosted services after the product transition

The native client controls presentation and native connections. The independently installable
runtime owns Harness execution, sessions, projects, local persistence and node supervision.
Hosted services own account authority and explicitly authorized hosted capabilities. Signing in
does not upload local files, projects or conversations, and local or SSH execution requires no
hosted account.

## Retained private components

| Component | Responsibility | Integration status |
| --- | --- | --- |
| `packages/ui` | Reusable primitives, assistant renderers, design tokens and component examples | Preserved; the public application has no private package dependency |
| `packages/auth` | Account presentation and credential workflows | Reusable library; requires an explicitly supplied host |
| `packages/api` | Account, passkey, membership and invitation HTTP contracts | Private account-service client, separate from the public runtime protocol |
| `packages/web` | Product website and newsletter | Preserved, with retired browser-application launch links removed |
| `packages/config` | Shared frontend tooling, fonts and styles | Preserved |
| `crates/core` | Account identity, membership authority and attributed domain facts | Preserved foundation for later collaboration |
| `crates/server` | Account sessions, credential verification, transactional email, memberships and invitations | Preserved private service; does not execute Harnesses or assistant turns |

The authentication host supplies an HTTP client, a dedicated query cache, a website origin,
navigation and navigation-error reporting. No client is created from build-time environment
variables. Credentials remain transient; account replacement cancels and clears private state.
Password authentication, email verification, recovery, magic links and browser passkey ceremonies
retain their one-shot proof handling and bounded recovery reads. A client consumes the library's
stylesheet and selects its own account surfaces and navigation.

Account completion and invitation landing pages are served by the account service. Credential
and protected HTTP operations trust its exact origin and one configured embedding-client origin;
protected mutations still require the session-specific CSRF token. Passkey ceremonies retain
their configured relying-party origin. Other account
surfaces require an embedding client. Extracting these components does not implement native account
enrollment, desktop cookie transfer or account-linked node pairing.

Native account credentials, renewal and revalidation belong to the public native core and its
persistent headless owner. The desktop gateway presents and forwards account operations; its
window lifetime does not own those credentials. CLI and runtime consumers can obtain current
authority without opening a graphical client. Interactive-user sessions and runtime machine
grants retain distinct scopes and protected storage.

## Retired implementation

The standalone browser application, its routes and deployment definitions have been removed.
The former private conversation store, execution coordinator, direct model adapters, model catalog,
selection preferences, streaming conversation API and corresponding development fixtures have also
been removed. Conversation execution belongs to the runtime and its Harness adapters.

The development launcher starts account services and disposable PostgreSQL without a browser app
or a synthetic inference provider. Frontend CI checks the authentication library, HTTP contracts,
shared UI and website. Backend CI validates account services; image publication remains an explicit
manual action. The coupled browser-app preview and backend deployment workflows have been retired.
No existing deployment, provider configuration, credential or persistent database was changed.

The current schema is intended for a fresh development database. Retired conversation migrations
are absent; an existing deployment may have a different migration ledger. Do not apply this schema
to existing data as an implicit upgrade. A future deployment must explicitly address its database
lifecycle and preserve user-owned data. Encrypted deployment credentials remain unchanged.

`FRONTEND_ORIGIN` must identify the approved host for account flows before deploying persistent
services. Historical provider provisioning records are operational evidence, not product configuration.

## Remaining work

- Extend the implemented [native account handoff and profiles](native-accounts.md) using the
  [integration plan](account-integration-plan.md). Complete production policy and portal coverage
  before deploying account onboarding. Browser cookies are not runtime credentials.
- Move or independently extract selected UI components into public source if the public client adopts
  them. It must continue to build without private repositories.
- Revalidate membership and invitation rules against the intended collaboration model. Retaining the
  foundation does not make the prior Workspace browser workflow the current product design.
- Implement account-linked enrollment, hosted relay routing, cloud allocation, collaboration
  authorization, synchronization, billing and entitlements as independently scoped hosted services.
- Revise the website's broader product illustrations and copy when those flows are established.
  Website publication is manual during this transition; no site deployment is part of this cleanup.
- Rebuild deployment and preview flows around the individual hosted services after account hosting,
  schema lifecycle and environment authority are defined.

## Documentation ownership

This document and all other product documentation belong under the client/runtime
repository's root `docs/` folder, tracked directly with that repository. There is no
separate documentation repository, mounted checkout, or pinned documentation revision.
Documentation edits follow the parent repository's normal review workflow. The hosted-service
repository remains independent and private and does not mount another copy. Both source
repositories must build without documentation contents.
