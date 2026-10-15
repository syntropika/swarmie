# Public documentation

The public Swarmie documentation site uses Fumadocs, Next.js, and MDX. Its content is in `content/docs`; final guide screenshots and schema downloads are in `public`. It builds as a static site with browser-local search and does not need a runtime, account credentials, or private source packages.

Documentation assets live under this directory: captures and the downloadable client example in `public/extensions`, monochrome logos in `public/brand`, and fonts in `public/fonts`. The standalone guide in `public/extensions-guide` shares the same captures and example with Fumadocs. The contribution demo has its own sources and build in `apps/desktop/web/demo`; the production desktop build contains neither fixtures nor documentation assets.

## Develop and build

Use Node.js 24 and npm. From the repository root:

```sh
npm --prefix apps/docs ci
npm --prefix apps/docs run dev
```

Open the development server's printed address. For a static build and preview:

```sh
npm --prefix apps/docs run build
npm --prefix apps/docs run preview
```

The preview serves `out` on port 3000. When opening it from another computer, use the server's reachable address; loopback refers to the computer opening the link.

```sh
npm --prefix apps/docs run check
```

Build and checks reject stale method references, schema downloads, and branding without rewriting committed files. After changing the canonical public contract or brandkit, update the generated files explicitly:

```sh
npm --prefix apps/docs run sync
npm --prefix apps/docs run test:generated
```

Update `packages/contracts/protocol.json` rather than editing the generated method table directly. Content validation checks internal links, slot/capture coverage, and accidental private operational text.

## Content boundary

Public pages describe user workflows, observable product behavior, supported contracts, installation, extension authoring, and clearly stated availability limits. Internal agent instructions, product/design plans, operational records, credentials, and deployment history are separate from this content tree. Private material must never be imported into a page loader, search index, static asset directory, or generated Markdown output.

Source documentation reflects the current development checkout. Examples must not imply published installers or registry packages unless a corresponding release actually exists. Keep persisted prose in English. Screenshots with simulated data must say so.

This public application lives in `apps/docs/`. Product and technical documentation is tracked in the repository-root `docs/` folder; public builds do not read it. Repository visibility and site deployment are separate actions from maintaining these public source files.

## Publishing

Set `DOCS_SITE_URL` to the canonical public origin at build time to enable absolute social-image metadata and the sitemap. Leave it unset for local previews; no deployment domain is assumed.

`out` contains the static HTML, assets, search data, Markdown exports, and LLM text indexes. Any static HTTP host can serve it. The documentation CI checks and archives this output; it does not publish it. Configure deployment and a public domain when publication is requested.

## Branding

The site uses canonical color tokens from `assets/brandkit/tokens/colors.css`. Its Fumadocs theme maps those colors to reading surfaces, navigation, links, and focus states in `app/global.css`. Explicit synchronization keeps `app/brand-tokens.css`, the monochrome logos, and the favicon identical to the official assets; builds and checks report drift.

Headings use the unmodified, self-hosted Manrope V5 variable font from [Michael Sharanda](https://www.sharanda.com/manrope). Body text and interface controls retain platform sans-serif typography. The font attribution and license reference are in `public/fonts/NOTICE.txt`.
