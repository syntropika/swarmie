# Swarmie brandkit

The maintained brandkit is `assets/brandkit`. It contains canonical vector geometry,
transparent raster exports, a vector logo-family sheet and an image-generated brand
presentation. There is no duplicate ZIP distribution in the source tree.

## Identity

The default identity is `logos/swarmie-ink-coral.svg`: an ink symbol, coral rounded-diamond
core and the original ink swarmie lettering. Use `logos/swarmie-white-coral.svg` on dark
backgrounds. Single-color ink and white variants remain available for monochrome use.
Corresponding standalone symbols live in `marks/`. Every SVG preserves the original
proportions; the core geometry is recorded in `source/logo-geometry.json`.

The full lockup includes its custom wordmark. Do not rebuild it from a symbol plus text,
stretch it, recolor its lettering or reintroduce the retired blue multicolor applications.
Leave clear space around it and use a neutral surface with sufficient contrast.

`BrandLogo` and `BrandMark` from `@swarmie/ui-web` load these assets using bundle-safe imports
for both server-rendered and browser interfaces. Both default to coral-core artwork;
`treatment="monochrome"` selects single-color reproduction and `tone="inverse"` selects
white artwork on an explicitly dark surface. Artwork is decorative within a named link or
control; an isolated logo needs an accessible name on its containing element.

## Color combinations

The shared [design reference](DESIGN.md) defines the complete role policy. The brandkit's
`tokens/colors.json` describes identity and editorial primitives; runtime themes remain
owned by `packages/config`.

| Base | Action | Optional accent | Example |
| --- | --- | --- | --- |
| Light neutral card, dark text | Black primary button, white label | Mint chip | A named project category beside a neutral form. |
| Dark neutral card, light text | White primary button, dark label | Dark lilac chip | A project label inside a dark settings card. |
| Neutral row | Gray secondary button | Peach avatar fallback | Initials distinguish a participant without implying status. |
| Neutral collection | Neutral controls | Mustard category label | A named collection marker with a non-color cue. |

Mint, lilac, peach and mustard identify categories, projects, groups, initials and illustration
details. Assignments are examples rather than mandatory domain meanings. Keep an assignment
stable within the view, label it and use the darker theme pair on dark surfaces. A pastel
should not become the canvas, the global primary action or a substitute for success, warning,
danger, focus or selection. The coral core remains the restrained identity accent.

The marketing footer uses warm black with one monochrome white full lockup at the far right. Existing blue illustration details remain valid editorial accents.

## Typography and component treatment

Google Sans is the interface and marketing family. Its self-hosted file, license and CSS
entry live in `packages/config/fonts`; the wordmark retains its original outlined lettering.
Native follows its platform configuration. The system monospace stack is reserved for code.

HeroUI owns web control geometry. The shared Medium radius base is 0.5rem. Primary actions
are monochrome, secondary actions gray, and tertiary actions transparent. Optional category
colors can tint chips and avatar fallback parts while retaining native sizes and spacing.
The generated presentation illustrates these relationships; source components and shared
tokens remain authoritative for implementation.

## Delivery and maintenance

- `logos/`: approved full lockups, SVG and PNG.
- `marks/`: approved standalone symbols and coral core, SVG and PNG.
- `source/`: canonical full logo and geometry data.
- `tokens/`: identity/editorial color primitives and usage metadata.
- `typography/`: reference to the maintained font and license.
- `boards/swarmie-logo-family.svg`: exact vector artwork in the four approved treatments.
- `boards/swarmie-brand-toolkit.png`: refreshed image-generated presentation of the current identity.
- `mascot/`: optional coral editorial mascot.

Change canonical SVG geometry only after an explicit identity decision. A generated raster
presentation is not a new master logo. Update metadata, maintained assets, consumers and
reference boards together. The documentation reader synchronizes its brand copies through
its existing `sync-brand.mjs` workflow; the website favicon uses the canonical coral mark.

## Design tooling

The shared reference is `docs/DESIGN.md`. Impeccable's derived metadata lives
beside it at `docs/.impeccable/design.json`; scoped composition lives in the
corresponding documentation folders. There are no root or application-local metadata copies.

Read these paths explicitly when the tool's default project discovery does not follow the
documentation layout. Do not recreate a root `.impeccable` folder or duplicate DESIGN files
to satisfy automatic discovery. Temporary critiques, generated drafts and screenshots belong
outside the source repository under the configured artifact root.
