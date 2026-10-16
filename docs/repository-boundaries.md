# Repository boundaries

## Ownership

The parent repository is the future open-source product. It contains the clients and the
independently installable execution runtime. This repository also owns the product website,
client account presentation, shared UI and public documentation site. The independent `swarmie-cloud` repository owns operator-managed services.
It is not a submodule or workspace member of the client/runtime repository. Both source
repositories remain private until publication is explicitly authorized. Documentation is
tracked in each owning source repository's root `docs/` folder: client/runtime guides here,
and hosted-service architecture, policies and plans in `swarmie-cloud`.

| Capability | Parent repository | Private repository |
| --- | --- | --- |
| Clients | UI, native host, connections, reconnection, local projections | Hosted APIs consumed by clients |
| Execution nodes | Runtime, execution policy, local credentials and persistence | Account-linked node registry and hosted discovery |
| Harness integration | Adapters, common capabilities, imported sessions | No private dependency required |
| Accounts | Login integration and secure client credential storage | Identity, recovery and account administration |
| Device linking | Pairing UI, runtime enrollment adapter, grant verification | Code redemption, grant issuance and hosted revocation |
| Connectivity | Direct and SSH connections, relay protocol adapter | Hosted rendezvous and relay service |
| Managed cloud | Execution runtime and public execution contract | Allocation, resource quotas, lifecycle and cleanup |
| Collaboration | Public contracts, client presentation, runtime enforcement | Membership, invitations, roles and hosted authorization |
| Documents and media | Editors, formats and local operations | Hosted storage, shared revisions and synchronization |
| Extensions | Extension API, local tools and Harness-based coordination | Optional hosted coordination and commercial services |
| Plans and billing | Plan and consumption presentation | Billing, credits, entitlements and commercial usage metering |

These are responsibility boundaries, not a requirement to deploy each capability as a
microservice. The account-linked pairing, relay, managed cloud and hosted collaboration
services remain future work. Account identity and native handoff follow the
[hosted-service architecture](https://github.com/syntropika/swarmie-cloud/blob/develop/docs/architecture.md)
in the private repository. Conversation execution belongs to the
independently installable runtime; clients observe it through the native gateway or explicit
browser adapter.

## Dependencies and builds

The client/runtime repository must install, compile, test, and operate without the
hosted-service source repository or documentation contents. It owns its Bun workspace,
lockfile, shared compiler defaults, formatter, linter, toolchain, and development recipes.
`packages/config` owns shared compiler settings, fonts, Tailwind foundations, and the
web/native token stylesheets. Its `tailwind/tooling-tokens.css` export combines both token
sets for shared tooling. `packages/ui/web` (`@swarmie/ui-web`) and `packages/ui/native`
(`@swarmie/ui-native`) are separate local workspace packages with independent dependencies,
compiler projects, and Storybooks. Native renderer and Uniwind tooling stay out of the web
package. The root workspace explicitly includes both UI packages; root checks and catalog
builds cover both. There is no separate UI-token package.

The hosted repository is autocontained and retains its own packages and dependency
configuration. Neither repository imports the other's source by relative path or shares a
local workspace. Public wire contracts remain the integration boundary. Future shared
package distribution must use explicit versions and is not part of this separation.
Documentation is not a Rust workspace package and does not supply build inputs.

Publicly consumed schemas and types remain source code in the parent. There is one authoritative
contract for each interface. Generated types do not imply that the desktop uses a language
SDK's transport: managed desktop communication passes through native IPC.

The source repositories have independent commit histories, branches, CI, and release
lifecycles. Client/runtime jobs must not automatically check out hosted-service source.
Hosted-service jobs run from their own root and need no client/runtime checkout.

## Documentation and data

Project manuals, architecture documents, specifications, plans, and component README files
belong in the owning source repository's root `docs/` folder. Client/runtime and shared
product guides are tracked here; private hosted-service architecture, policies and delivery
plans are tracked in `swarmie-cloud/docs/`. Do not duplicate those private documents here.
There is no documentation gitlink, separate documentation repository, mounted checkout or
build-time documentation dependency between repositories. Shared guides live directly in
`docs/`; runtime, desktop, mobile, SDK, extensions and testing use direct category folders.
License files remain alongside source packages. Distribution packages do not require or
copy project documentation; source builds remain independent of its contents.

The root `AGENTS.md` is this repository's canonical agent policy; `docs/AGENTS.md` is a
relative symlink to it. The independent
hosted repository owns its own root agent configuration. Each source repository directs
agents to its own root policy.

Signing in or enrolling a node does not upload all local projects, files or conversations.
Hosted features require explicit transfer and authorization. The runtime controls local
execution; hosted services control account authority, memberships, cloud allocation and plans.
Secrets and production data must not be committed to any repository.

Moving documentation does not change repository visibility or erase previous Git
history. Historical content must be reviewed before any future change to public visibility;
publication remains separate work and is not authorized by this reorganization.

## Public source license

The parent repository and its first-party crates, clients and runtime packages use Apache-2.0.
First-party attribution is `Copyright 2026 Syntropika`. Each distributed SDK includes the Apache
license and the first-party attribution notice.
Application and runtime archives include these files as well. Vendored and external dependencies
retain their upstream licenses and attribution requirements. This does not change the visibility
of any repository or grant a new license to private hosted-service implementations.
