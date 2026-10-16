---
name: Swarmie native mobile
description: Monochrome native controls and focused account access.
colors:
  native-canvas: "#ffffff"
  native-ink: "#1a1c1f"
  native-muted: "#606568"
  native-control: "#f3f3f3"
  native-control-hover: "#e8e8e8"
  native-border: "#d9d9d9"
  native-field: "#eeeeee"
  native-field-focus: "#e9e9e9"
typography:
  heading:
    fontFamily: system-ui
    fontSize: 2.125rem
    fontWeight: 700
    lineHeight: 1.15
    letterSpacing: -0.025em
  input:
    fontFamily: system-ui
    fontSize: 1.25rem
    lineHeight: 1.4
  field-label:
    fontFamily: system-ui
    fontSize: 1.25rem
    fontWeight: 500
    lineHeight: 1.4
rounded:
  pill: calc(infinity * 1px)
  code-cell: 1rem
spacing:
  native-control: 3.5rem
  native-field: 3.75rem
  native-brand-width: 14rem
  native-brand-height: 3.75rem
  native-content: 28rem
components:
  button-primary:
    backgroundColor: "{colors.native-ink}"
    textColor: "{colors.native-canvas}"
    rounded: "{rounded.pill}"
    padding: 1rem 1.5rem
  button-secondary:
    backgroundColor: "{colors.native-control}"
    textColor: "{colors.native-ink}"
    rounded: "{rounded.pill}"
    padding: 1rem 1.5rem
  field:
    backgroundColor: "{colors.native-field}"
    textColor: "{colors.native-ink}"
    typography: "{typography.input}"
    rounded: "{rounded.pill}"
    padding: 1rem 1.5rem
  field-focus:
    backgroundColor: "{colors.native-field-focus}"
    textColor: "{colors.native-ink}"
    typography: "{typography.input}"
    rounded: "{rounded.pill}"
    padding: 1rem 1.5rem
---

# Design System: Swarmie native mobile

## Overview

Native account access uses a sparse monochrome surface, the canonical brandkit logo,
system typography and generous space between the introduction, form and bottom actions.
The accepted direction is restrained, legible and familiar to native mobile use.

This document records the native implementation in `apps/mobile/src/account` and
`packages/ui/native/src`. Native presentation is exported through `@swarmie/ui-native`
and checked by its own compiler project and independent Storybook. The incumbent web
design remains in `packages/ui/web` (`@swarmie/ui-web`).

Shared native wrappers currently adapt HeroUI Native 1.0.10 `Button`, `Input`, `Label`,
and `TextField` as a reversible trial. `NativeProvider` contains vendor setup and honors
reduced motion. The application owns gesture, safe-area, and keyboard roots. This trial
does not establish a permanent vendor choice.

The presentation port is currently unconfigured for live account access. An explicit
development preview uses in-memory fixtures; it does not authenticate an account,
send verification email or invoke a native passkey ceremony. Native visual,
keyboard, accessibility and device qualification remain pending.

## Colors

The native palette is owned by `packages/config/tailwind/native-tokens.css`;
`packages/config/tailwind/native-tokens.css` owns the native semantic mappings above,
exported as `@swarmie/config/tailwind/native-tokens.css`. Mobile also loads the config's
web token stylesheet for existing runtime screens; account onboarding uses native tokens.
The config's `hero-native.css` maps vendor semantic roles to those tokens and preserves
the approved light monochrome appearance in either system mode. Native utilities are
emitted after vendor component rules so overrides retain their effect in browser previews.

- **Primary:** native ink fills the main action; canvas supplies its text.
- **Secondary:** native control fills alternative actions; native control hover supplies
  the vendor's neutral interaction color. Fields use their distinct native field fill,
  with a subtle neutral fill change on focus.
- **Neutral:** canvas is the screen background, ink is heading and label text,
  and muted is supporting copy, email context and text actions. Native border is
  available in the shared palette; account text fields have no border, including
  during autofocus.

**The Monochrome Rule.** Keep account onboarding, selection, focus and feedback
within the neutral palette. Errors need explicit text, not an added accent color.

## Typography

Use the platform system font through the native font token. The heading token defines
the prominent title; body copy uses the standard base size with a six-unit line height.
Button labels use the base size and semibold weight. Supporting actions and notices use
the small size; legal copy uses the extra-small size. Persistent field labels and typed
values share the larger input type token. Field labels use native ink with medium weight
(500). Preserve text wrapping and native text scaling.

Mobile supports English, Spanish, French, German, Portuguese, Italian, Japanese, and
Simplified Chinese through consumer-owned typed catalogs. Translate labels, supporting
copy, feedback, and accessible names together. Preserve user and agent content as authored.

## Layout

Account screens fill the available safe area with a white canvas and a centered column
bounded by the native content-width token. The frame distributes brand and heading,
form content, and bottom actions vertically, with flexible space before the footer.
There is no enclosing gray card.

Use a keyboard-avoiding, scrollable frame so short screens and visible keyboards can
reach every field and action. Keep bottom actions in the shared footer position;
allow content to scroll rather than compressing controls. Native brand geometry uses
the shared width and height tokens with contained aspect ratio and available-width
constraints.

Allow translated headings, action labels, and notices to wrap within the existing column.
Longer copy may extend vertical scrolling on narrow screens; do not shrink the shared
controls or introduce horizontal overflow. Language choice belongs in Settings and the
development preview guide, without adding another control to account entry.

## Elevation & Depth

Account onboarding is flat, without shadows or floating panels. Gray control fills,
spacing and type hierarchy distinguish interactive elements from the white canvas.

## Shapes

Buttons and text fields are full pills. Verification uses six softly rounded cells.
Primary and secondary buttons use the native minimum control-height token; text fields
use the taller native field-height token. Text actions retain a minimum touch area of
eleven spacing units in each axis.

## Components

### Brand

Bundle the original monochrome ink logo from `assets/brandkit` through the native image
primitive. Preserve its geometry and accessible Swarmie name. Do not redraw the mark
or change the shared web asset to produce native presentation.

### Buttons and actions

`NativeButton` exposes primary and secondary tones through the vendor button. Secondary
actions are filled gray pills. The vendor supplies press feedback and disabled treatment;
the shared provider disables animations for reduced motion. Loading disables the button
and exposes busy state; the consumer supplies the pending label.

Buttons and text actions provide visible neutral focus outlines. `NativeTextAction`
uses muted underlined text for contextual actions such as changing email, password reset
and code resend.

### Fields and feedback

`NativeField` composes vendor `TextField`, `Label`, and `Input` while keeping its
medium-weight ink label outside the gray pill. At the base native
scale, its minimum height is 60 logical units, left padding is 24, and labels and values use
a font size of 20. It has no border or outline, including during autofocus;
focus uses the subtle neutral fill change and native caret. The Android underline is
suppressed. Placeholder text defaults to the muted token through the native
`placeholderTextColor` prop, resolved with `useCSSVariable`. The optional trailing slot
reserves extra input padding to keep typed text clear of its action.

`NativeIconAction` supplies a shared button target (48 square) with a required accessible
label and disabled state. The account-owned `PasswordField` composes it with `NativeField`
and the app's existing SVG `Icon` component. Muted eye and eye-off icons (28 square) are
hidden from accessibility navigation; the button exposes localized show/hide password labels.
The account component owns the `secureTextEntry` toggle, field labels and password autofill
policy. Presentation does not define identity-service password policy.

`NativeCodeField` retains six visual cells backed by one numeric vendor `Input` with one-time-code
autofill. Only that input participates in accessibility navigation; decorative cells
are hidden. It accepts at most six digits and marks the focused cell with an ink border.
The consumer must supply its localized `accessibilityLabel`; stories use an English fixture label.
It does not use the vendor's segmented `InputOTP` component.
`NativeNotice` supplies textual, politely announced feedback.

### Account screens

The initial screen combines welcome and account access around one email field. A trusted
lookup determines account creation or the existing account's supported sign-in methods.
Prefer passkey sign-in when available, with password as a secondary action; show the
inverse alternative on the password screen only when supported.

The device-unlock explanation belongs only to the login passkey screen. The login
password screen contains its password field and recovery action without that explanation.
Account creation asks for a password, then a six-digit email verification code, followed
by optional passkey enrollment with a skip action. Enrollment may explain creating and
using a passkey. Future credential ceremonies must use the operating system interface.

## Do's and Don'ts

- **Do** reuse the native token stylesheet and shared native primitives.
- **Do** preserve safe areas, visible labels, native input hints and announced feedback.
- **Do** distinguish preview transitions from real identity and credential operations.
- **Don't** surround the onboarding form with a gray card or add colored accents.
- **Don't** infer account existence, available methods or successful authentication locally.
- **Don't** duplicate the passkey device-unlock explanation on the login password screen.
- **Don't** treat a web preview or static export as native device qualification.
