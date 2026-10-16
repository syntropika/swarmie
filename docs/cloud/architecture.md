# Hosted-service architecture

The native client controls presentation and native connections. The independently installable
runtime owns Harness execution, sessions, projects, local persistence and node supervision.
Hosted services own account authority and explicitly authorized hosted capabilities. Signing in
does not upload local files, projects or conversations, and local or SSH execution requires no
hosted account.

## Source ownership

| Component | Responsibility | Integration status |
| --- | --- | --- |
| `packages/ui/web` in `swarmie` | Reusable primitives, design tokens and component examples | Shared HeroUI primitives; no private package dependency |
| `apps/web` in `swarmie` | Product website and newsletter | Marketing website; separate from runtime client workflows |
| `packages/config` in `swarmie` | Shared frontend tooling, fonts and styles | Shared root workspace |
| `crates/core` | Account identity, membership authority and attributed domain facts | Hosted identity and collaboration foundation |
| `crates/server` | Account sessions, credential verification, transactional email, memberships and invitations | Hosted account authority; no Harness execution |

The TypeScript applications, shared packages, and Storybooks share the `swarmie` root
workspace, lockfile, and checks. Cloud retains Rust services and backend operations.

Account browser authentication, completion pages, invitation acceptance and explicit native
handoff belong to the hosted account service. Password, passkey, email verification and
recovery endpoints retain their server-side proof, origin and CSRF rules. A complete
credential-only browser portal remains separate work; native onboarding must use an
actually available hosted ceremony rather than assuming a local TypeScript library exists.
See [native account grants and profiles](native-accounts.md) for supported handoff endpoints
and production qualification limits.

Native account credentials, renewal and revalidation belong to the public native core and its
persistent headless owner. The desktop gateway presents and forwards account operations; its
window lifetime does not own those credentials. CLI and runtime consumers can obtain current
authority without opening a graphical client. Interactive-user sessions and runtime machine
grants retain distinct scopes and protected storage.

## Execution and development boundaries

Conversation execution belongs to the independently installable runtime and its Harness
adapters. Hosted services do not provide a separate conversation store, model execution
coordinator or browser application. The desktop's explicit browser development adapter
uses the current native application workflows and runtime protocol.

In the hosted-service repository, `just dev` starts account services and disposable
PostgreSQL from `config/local.toml`, with memory identity and email providers. It uses
Bash and Docker Compose, requires no Node runtime, refuses an existing project or
concurrent launcher, and cleans up only its own API and database on exit. A custom
profile and matching PostgreSQL port can be passed as positional arguments; ports are
explicit rather than automatically reassigned.

Frontend CI belongs to `swarmie`; backend CI validates account services in `swarmie-cloud`.
Image publication is an explicit manual action. The development schema targets a fresh
database. Applying it to an existing deployment requires a separate database lifecycle
decision that preserves user-owned data. `FRONTEND_ORIGIN` identifies the approved host
for account flows. Provider provisioning records remain outside project documentation.

## Remaining work

- Extend the implemented [native account handoff and profiles](native-accounts.md) using the
  [integration plan](account-integration-plan.md). Complete production policy and portal coverage
  before deploying account onboarding. Browser cookies are not runtime credentials.
- Revalidate membership and invitation rules against the intended collaboration model before
  treating retained account-domain types as qualified collaboration behavior.
- Implement account-linked enrollment, hosted relay routing, cloud allocation, collaboration
  authorization, synchronization, billing and entitlements as independently scoped hosted services.
- Rebuild deployment and preview flows around the individual hosted services after account hosting,
  schema lifecycle and environment authority are defined.

## Documentation ownership

This document and all other product documentation belong under the client/runtime
repository's root `docs/` folder, tracked directly with that repository. There is no
separate documentation repository, mounted checkout, or pinned documentation revision.
Documentation edits follow the parent repository's normal review workflow. The hosted-service
repository remains independent and private and does not mount another copy. Both source
repositories must build without documentation contents.
