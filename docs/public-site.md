# Public documentation

The public Swarmie documentation site uses Fumadocs, Next.js, and MDX. Its content is in `apps/docs/content/docs`; final guide screenshots and schema downloads are in `apps/docs/public`. It builds as a static site with browser-local search and does not need a runtime, account credentials, or private source packages.

Documentation assets live under `apps/docs/public`: captures and the downloadable client example in `extensions`, canonical logos in `brand`, and the favicon. The standalone guide in `apps/docs/public/extensions-guide` shares the same captures and example with Fumadocs. The contribution demo has its own sources and build in `apps/desktop/src/demo`; the production desktop build contains neither fixtures nor documentation assets.

## Develop and build

Use Node.js 24 and the pinned Bun toolchain. From the repository root:

```sh
mise exec -- just setup
mise exec -- just --command bun run --cwd apps/docs dev
```

Open the development server's printed address. For a static build and preview:

```sh
mise exec -- just --command bun run --cwd apps/docs build
mise exec -- just --command bun run --cwd apps/docs preview
```

The preview serves `apps/docs/out` on port 3000. When opening it from another computer, use the server's reachable address; loopback refers to the computer opening the link.

```sh
mise exec -- just --command bun run --cwd apps/docs check
```

Build and checks reject stale method references, schema downloads, and branding without rewriting committed files. After changing the canonical public contract or brandkit, update the generated files explicitly:

```sh
mise exec -- just --command bun run --cwd apps/docs sync
mise exec -- just --command bun run --cwd apps/docs test:generated
```

Update `packages/contracts/protocol.json` rather than editing the generated method table directly. Content validation checks internal links, slot/capture coverage, and accidental private operational text.

## Content boundary

Public pages describe user workflows, observable product behavior, supported contracts, installation, extension authoring, and clearly stated availability limits. Internal agent instructions, product/design plans, operational records, credentials, and deployment history are separate from this content tree. Private material must never be imported into a page loader, search index, static asset directory, or generated Markdown output.

Source documentation reflects the current development checkout. Examples must not imply published installers or registry packages unless a corresponding release actually exists. Keep persisted prose in English. Screenshots with simulated data must say so.

This public application lives in `apps/docs/`. Client/runtime product and technical documentation
is tracked in this repository's root `docs/` folder; public builds do not read it. Private
hosted-service documentation belongs to `swarmie-cloud/docs/` and must not be copied into this
application. Repository visibility and site deployment are separate actions from maintaining
these public source files.

## Publishing

Set `DOCS_SITE_URL` to the canonical public origin at build time to enable absolute social-image metadata and the sitemap. Leave it unset for local previews; no deployment domain is assumed.

`apps/docs/out` contains the static HTML, assets, search data, Markdown exports, and LLM text indexes. Any static HTTP host can serve it. The documentation CI checks and archives this output; it does not publish it. Configure deployment and a public domain when publication is requested.

## Branding

The site uses canonical color tokens from `assets/brandkit/tokens/colors.css`. Its Fumadocs theme maps those colors to reading surfaces, navigation, links, and focus states in `src/app/global.css`. Explicit synchronization keeps `src/app/brand-tokens.css`, the canonical coral-core logos, and the favicon identical to the official assets; builds and checks report drift.

Headings, body text and interface controls use the shared self-hosted Google Sans entry from `@swarmie/config/fonts`. The maintained font and its license are in `packages/config/fonts`. Canonical logo lettering remains outlined artwork.
