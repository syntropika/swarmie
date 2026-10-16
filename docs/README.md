# Swarmie documentation

Client/runtime documentation lives in this folder and is tracked with this source repository.
Hosted-service documentation is maintained in the independent private
[`swarmie-cloud` repository's `docs/` folder](https://github.com/syntropika/swarmie-cloud/tree/develop/docs).
The runtime, clients and distribution packages build without either documentation tree.

## Shared guides

- [Product direction](PRODUCT.md) and [domain vocabulary](CONTEXT.md).
- [Design system](DESIGN.md), [design principles](design-principles.md) and [brandkit](brandkit.md).
- [Frontend engineering](frontend.md).
- [Repository boundaries](repository-boundaries.md) and [separation status](separation-status.md).
- [Agent instructions](AGENTS.md).

## Areas

| Area | Start here |
| --- | --- |
| Runtime | [Overview](runtime/README.md), [architecture](runtime/architecture.md), [connectivity](runtime/runtime-connectivity.md) |
| Desktop | [Application guide](desktop/README.md), [composition](desktop/DESIGN.md), [product context](desktop/PRODUCT.md) |
| Mobile | [Foundation](mobile/foundation.md), [design](mobile/DESIGN.md), [dependencies](mobile/dependencies.md), [orchestration](mobile/orchestration.md), [iOS builds](mobile/ios-builds.md), [signing](mobile/apple-signing.md), [OTA updates](mobile/ota-updates.md) |
| SDK | [Overview](sdk/README.md), [native boundary](sdk/native-client.md), [TypeScript](sdk/typescript.md), [Python](sdk/python.md), [Go](sdk/go.md), [Swift](sdk/swift.md) |
| Extensions | [API](runtime/extension-api.md), [Git](extensions/git.md), [permissions](extensions/permissions.md), [resources](extensions/resources.md), [workspace](extensions/workspace.md) |
| Testing | [Fixtures and qualification](testing/README.md) |
| Public documentation site | [Development, content and publishing](public-site.md) |
| Hosted services (private repository) | [Architecture](https://github.com/syntropika/swarmie-cloud/blob/develop/docs/architecture.md), [accounts](https://github.com/syntropika/swarmie-cloud/blob/develop/docs/account-integration.md) |
| Legal | [Third-party notices](legal/THIRD_PARTY_NOTICES.md) |

Keep category folders shallow. Shared references belong directly here; application or
capability guides belong to their named category. Avoid duplicating source-tree levels
such as `apps`, `packages`, `crates` or nested `docs` folders solely to hold a README.
Impeccable's derived design metadata lives at `.impeccable/design.json` beside the shared
DESIGN reference. Temporary reviews and operational artifacts remain outside the repository.
