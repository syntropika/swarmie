---
name: Swarmie
description: Neutral foundations, expressive category accents, and shared HeroUI controls.
colors:
  background: "var(--background)"
  foreground: "var(--foreground)"
  surface: "var(--surface)"
  surface-secondary: "var(--surface-secondary)"
  accent: "var(--accent)"
  accent-foreground: "var(--accent-foreground)"
  ink: "#1A1C1F"
  white: "#FFFFFF"
  coral: "#DDA99F"
  mint: "#CDE9DE"
  lilac: "#DED9F2"
  peach: "#FFD2BE"
  mustard: "#F6E6A6"
  editorial-blue: "#4C6486"
typography:
  display:
    fontFamily: '"Google Sans", ui-sans-serif, system-ui, sans-serif'
    fontSize: "clamp(2.5rem, 5vw, 4.5rem)"
    fontWeight: 600
    lineHeight: 1.05
    letterSpacing: "-0.035em"
  headline:
    fontFamily: '"Google Sans", ui-sans-serif, system-ui, sans-serif'
    fontSize: "clamp(1.75rem, 3vw, 2.5rem)"
    fontWeight: 600
    lineHeight: 1.15
  title:
    fontFamily: '"Google Sans", ui-sans-serif, system-ui, sans-serif'
    fontSize: "1.5rem"
    fontWeight: 500
    lineHeight: 1.25
  body:
    fontFamily: '"Google Sans", ui-sans-serif, system-ui, sans-serif'
    fontSize: "1rem"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: '"Google Sans", ui-sans-serif, system-ui, sans-serif'
    fontSize: "0.875rem"
    fontWeight: 500
rounded:
  medium-control: "0.5rem"
  marketing-card: "24px"
  marketing-shell: "32px"
spacing:
  unit: "0.25rem"
  small: "0.5rem"
  medium: "1rem"
  large: "1.5rem"
  section: "3rem"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.accent-foreground}"
---

# Swarmie design system

## Overview

**Creative North Star: "A quiet place for shared work."**

Swarmie uses calm neutral foundations so people can follow work, make decisions and stay
oriented. Black, white and gray establish hierarchy. A small family of muted category
colors adds warmth and helps distinguish content without competing with the next action.
The canonical black symbol, coral core and custom swarmie lettering anchor the identity.

This is the shared visual reference for web and desktop. Native keeps its platform renderer
and its separately qualified interaction contracts. The website uses the same typography,
brand artwork and control primitives while retaining its editorial composition, illustrations,
pastel accents and warm black footer. Application-specific composition is documented in
[desktop design](desktop/DESIGN.md) and [native design](mobile/DESIGN.md).

**Key characteristics:**

- Neutral component depth with clear foreground/background pairs.
- Original black or white logo artwork with a restrained coral core.
- Google Sans for interface and marketing copy; original outlined artwork for the wordmark.
- HeroUI geometry, behavior and compound APIs, composed through shared atoms.
- Optional mint, lilac, peach and mustard category accents with readable labels.

Implementation values belong to `packages/config`; this document describes their roles.
The frontmatter's semantic CSS references resolve through the current shared theme, rather
than freezing a second runtime palette. Brand primitives match `assets/brandkit`.

## Colors

**The Neutral Foundation Rule.** Canvas, component surfaces, fields, text and ordinary
actions stay within the approved grayscale. Decorative colors identify content, not the
importance of an arbitrary button.

| Role | Application |
| --- | --- |
| `background` / `foreground` | Overall canvas and primary text. |
| `surface` / `surface-foreground` | A distinct component plane, such as a card. |
| `surface-secondary` / paired foreground | Nested or secondary content within a component. |
| `overlay` / `overlay-foreground` | Floating menus, popovers and dialogs. |
| `field-background` / `field-foreground` | Inputs and other editable fields. |
| `muted` | Supporting text with verified contrast against its actual surface. |
| `accent` / `accent-foreground` | Monochrome primary actions and selection. |
| `default` / `default-foreground` | Neutral secondary controls, distinct from their containing surface. |
| `border`, `separator`, `focus` | Boundaries, divisions and keyboard focus. |

Dark depth uses `#181818` for background, `#212121` for surface and `#282828` for secondary
surface. These describe hierarchy across components, not fixed assignments to a sidebar
or composer. Secondary controls use their own lighter neutral pair so they remain visible
on cards. Light neutral roles and their state calculations follow the shared HeroUI preset.

**The Category Accent Rule.** Mint, lilac, peach and mustard may distinguish projects,
categories, initials, small content markers and illustration details. A category keeps its
chosen color within the current view and still has a name, icon or other non-color cue.
The example categories are illustrative; no hue has a mandatory product-domain meaning.

| Accent | Light surface | Dark surface | Example use |
| --- | --- | --- | --- |
| Mint | `#CDE9DE` | `#263E34` | A named category chip or avatar fallback. |
| Lilac | `#DED9F2` | `#363047` | A project label or diagram group. |
| Peach | `#FFD2BE` | `#473027` | A content tag or illustration detail. |
| Mustard | `#F6E6A6` | `#433D24` | A collection marker or category tag. |

A neutral dark card can contain white text, a white primary action and one lilac category
chip. A light card can use dark text, a black primary action and one mint category chip.
Tinted avatar fallbacks carry initials; a tint never replaces the person's name. Small
category chips may use primary, secondary, soft or tertiary treatment without resizing
HeroUI's geometry. Broad pastel planes are editorial illustration treatments, not the
application's default surfaces.

**The Meaning Rule.** Decorative mint is not success and decorative mustard is not warning.
Success, warning, danger, information, selection, focus and permissions retain their own
semantic contracts. Destructive actions use the solid danger pair; validation uses readable
danger text. Every important state also has wording or iconography. Do not recolor primary
actions, selected controls or error messages according to a nearby category.

Coral `#DDA99F` is the identity accent in the logo's core. The footer uses warm black `#181818` with white artwork. Existing blue editorial details
remain valid illustration treatments; blue is not a multicolor logo application or the
global primary control fill.

Normal text needs at least 4.5:1 contrast, large text 3:1, and meaningful control boundaries
and focus indicators 3:1. Check actual combinations in each theme. Pastel fills normally
pair with dark text in light mode and a readable light foreground in dark mode.

## Typography

Use the self-hosted Google Sans entry from `@swarmie/config/fonts`. Headings, controls and
reading share the same family. The system monospace stack is reserved for code and data
that benefits from aligned characters. Native typography remains platform-owned.

The product roles are display, headline, title, body, UI, label and caption from shared
configuration. Web and desktop do not maintain separate compact type scales. Use HeroUI's
native control sizes when appropriate; allow long labels to wrap. Reading copy uses a
bounded measure and relaxed line spacing. Marketing retains its existing fluid display
and section-heading roles rather than applying control typography to editorial headlines.

**The Artwork Rule.** The swarmie wordmark is the custom lettering in the canonical SVG.
Never recreate it by typing the name in Google Sans or another font. Keep the symbol,
coral core, lettering, spacing and proportions together in the full lockup.

## Layout

Components adapt to their available container; page shells use viewport breakpoints when
the viewport owns the layout. Follow the responsive reference in Foundations. The desktop
rail and resizable navigation compose the shared primitives without introducing a second
control collection. Dense information can use layout grouping and progressive disclosure
without forcing smaller primitive padding, heights or line spacing.

Use the shared spacing rhythm and upstream defaults. Reading regions usually stay near
65-75 characters. The marketing page keeps its existing bounded content width, generous
section spacing and responsive illustration layout. Layout classes may arrange controls;
they do not redefine the dimensions of those controls.

## Elevation & Depth

Use semantic surface layers to express containment. Overlay, field and surface shadows
come from HeroUI's shared theme and the Foundations Shadows reference. Do not invent
component-specific shadows, translucent stacks or extra backgrounds to imitate depth.
Focus is an interaction indicator rather than decorative elevation.

The three dark neutral layers are intentionally warm and lifted from pure black. Preserve
their distinction and keep neutral secondary controls readable on their surrounding plane.
Pastel category fills are optional content accents inside that hierarchy.

## Shapes

The shared preset uses HeroUI's Medium radius base: `--radius` and `--field-radius` are
`0.5rem`. Buttons retain the vendor's derived `rounded-3xl` shape, so their ends are visibly rounded;
fields use the shared field radius. Components retain the vendor's padding, sizes and shadows.
The existing marketing shell and illustration containers keep their larger editorial
radii; they do not redefine the primitive radius scale. Native shapes follow the separately
approved native contract.

Canonical logo streams, rounded diamond geometry and original lettering are artwork.
Their SVG viewBox and aspect ratio must remain intact. Use the inverse white/coral lockup
on dark backgrounds and monochrome variants where reproduction requires one color.

## Components

Atoms expose HeroUI primitives through `@swarmie/ui-web`, retaining their props, named parts,
render callbacks, refs, keyboard behavior and focus management. Button adds announced
loading state and excludes the ghost variant. Link uses text-only foreground feedback.
Chip extends the vendor color prop with the four optional category accents. Other color
applications use approved tokens on the vendor parts without changing geometry.

Primary buttons are black with white labels in light mode and light with dark labels in
dark mode. Secondary buttons use neutral gray. Tertiary actions retain transparent fills
with neutral interaction feedback. Status variants communicate status, not category.
Use Select, ComboBox, TextField, Modal and Dropdown directly for their established roles.

Molecules add behavior absent from the primitives, such as responsive/resizable
NavigationShell and VirtualList. There is no separate Workbench or Assistant component
library, density scale or palette. Application features own content, workflows and layout.
The documentation reader retains its reading framework and maps the shared identity into it.

Motion communicates a change of state or continuity, uses the vendor's established timing
and respects reduced motion. Use the shared icon exports for interface actions; brand and
provider artwork remain their own assets. Icons need meaningful accessible actions.

Foundations directly contains Overview, Brand, Responsive Design, Typography, Colors,
Spacing, Shadows, Motion and Icons. It has no Design Tokens catalog or separate Components
and Surfaces reference pages. The official component stories demonstrate vendor behavior;
our tests cover extensions and application integration rather than retesting the library.

## Do's and Don'ts

- Do keep neutral depth, primary actions and semantic status roles consistent.
- Do use category accents selectively with labels and stable assignments.
- Do keep the canonical wordmark and choose the correct light/dark artwork.
- Do preserve the website's established composition and editorial colors.
- Do verify contrast, keyboard behavior, responsive layout and asset loading in SSR.
- Do keep runtime tokens in `packages/config` and reusable DOM controls in `packages/ui/web`.
- Don't use category color as the only indicator of status, identity or selection.
- Don't introduce another primitive library, compact scale or per-application web theme.
- Don't redraw the logo, revive blue multicolor logo applications or add ghost buttons.
- Don't override HeroUI dimensions to make a component look more bespoke.
- Don't treat a generated brand presentation as the authoritative source for vector geometry.
