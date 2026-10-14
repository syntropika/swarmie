import type { Meta, StoryObj } from "@storybook/react-vite";

import { Avatar } from "./atoms/avatar";
import { Badge } from "./atoms/badge";
import { Button } from "./atoms/button";
import { Input } from "./atoms/input";
import { Separator } from "./atoms/separator";
import { cn } from "./classes";

const swatches = [
  ["Canvas", "bg-canvas text-ink", "#F4F1EC", "#181818"],
  ["Surface", "bg-surface text-ink", "#FCFAF7", "#232323"],
  ["Primary", "bg-primary text-on-primary", "#4C6486", "#AAC1E0"],
  ["Mint", "bg-mint text-ink", "#CDE9DE", "#263E34"],
  ["Peach", "bg-peach text-ink", "#FFD2BE", "#473027"],
  ["Butter", "bg-butter text-ink", "#F6E6A6", "#433D24"],
  ["Lilac", "bg-lilac text-ink", "#DED9F2", "#363047"],
  ["Ink", "bg-ink text-surface", "#272B33", "#F3F3F1"],
] as const;

const semanticSwatches = [
  ...swatches,
  ["Rail", "bg-rail text-ink", "#E9E6E0", "#111111"],
  ["Muted surface", "bg-surface-muted text-ink", "#EFEEE9", "#2D2D2D"],
  ["Primary soft", "bg-primary-soft text-primary", "#F5F4F2", "#2D2D2D"],
  ["Primary hover", "bg-primary-hover text-on-primary", "#3C506D", "#BDD0E9"],
  ["Footer", "bg-footer text-footer-text", "#4C6486", "#111111"],
  ["Inverse", "bg-inverse text-inverse-text", "#272B33", "#111111"],
  ["Success soft", "bg-success-soft text-success", "#E5F2E9", "#233B30"],
  ["Attention soft", "bg-attention-soft text-attention", "#FFF0C7", "#433A20"],
  ["Danger soft", "bg-danger-soft text-danger", "#FBEAEC", "#4A2B30"],
] as const;

const meta = {
  title: "Foundations/Swarmie",
  parameters: { layout: "fullscreen" },
} satisfies Meta;

export default meta;
type Story = StoryObj<typeof meta>;

export const ProductTypography: Story = {
  render: () => (
    <main className="mx-auto grid max-w-3xl gap-6 p-4 sm:p-6">
      <header>
        <h1 className="text-title">Product typography</h1>
        <p className="mt-2 text-body">Compact controls and comfortable reading.</p>
      </header>
      <section className="grid gap-3 rounded-panel bg-surface p-4">
        <h2 className="text-ui font-medium">Interface · 14/20</h2>
        <div className="flex flex-wrap gap-2">
          <Button>New conversation</Button>
          <Button variant="secondary">Suggested</Button>
          <Button variant="ghost">More options</Button>
        </div>
        <label htmlFor="typography-search" className="text-ui font-medium">
          Interaction · 16/24
        </label>
        <Input id="typography-search" placeholder="Search your workspace" />
      </section>
      <article className="grid gap-3 rounded-panel bg-surface p-4">
        <h2 className="text-document">A place for the next idea</h2>
        <p className="text-content leading-content">
          Document text uses the same quiet 14px scale as the interface, with a more generous line
          height. A useful note gives the reader room to follow the thought, while the controls stay
          compact and easy to scan.
        </p>
        <h3 className="text-body font-medium">Keep the next step clear</h3>
        <p className="text-content leading-content">
          Interface, reading, and code have independent scales. Each role can be adjusted without
          changing the others.
        </p>
        <pre className="overflow-x-auto rounded-panel bg-surface-muted p-3 font-mono text-code leading-code">
          const nextStep = &quot;Review the proposal&quot;;
        </pre>
      </article>
    </main>
  ),
};

export const SharedStudio: Story = {
  render: () => (
    <main className="mx-auto max-w-5xl px-6 py-12 sm:px-12">
      <p className="mb-10 text-2xl font-semibold tracking-tight">
        swarmie
        <span className="ml-3 text-ui font-normal tracking-normal text-text-muted">
          Design foundations
        </span>
      </p>
      <h1 className="max-w-3xl text-display">
        Good work,
        <br />
        in good company.
      </h1>
      <p className="mt-6 max-w-xl text-lg leading-relaxed text-text-muted">
        A shared place to bring people, agents, and work together. Warm surfaces, clear actions, and
        room to participate.
      </p>
      <div className="mt-8 flex flex-wrap gap-3">
        <Button>Invite people</Button>
        <Button variant="secondary">View the workspace</Button>
      </div>
      <section className="mt-16" aria-labelledby="palette-heading">
        <h2 id="palette-heading" className="text-title">
          A warm foundation
        </h2>
        <p className="mt-2 text-ui text-text-muted">
          Blue gives actions direction. Soft color adds personality without carrying status.
        </p>
        <div className="mt-6 grid grid-cols-2 gap-3 sm:grid-cols-4">
          {swatches.map(([name, background, light, dark]) => (
            <div
              key={name}
              className={cn(
                "flex min-h-28 flex-col justify-between rounded-panel border border-transparent p-4",
                background,
              )}
            >
              <span className="text-ui font-medium">{name}</span>
              <span className="font-mono text-xs">
                Light {light}
                <br />
                Dark {dark}
              </span>
            </div>
          ))}
        </div>
      </section>
      <section className="mt-12" aria-labelledby="type-heading">
        <h2 id="type-heading" className="text-title">
          Clear, open typography
        </h2>
        <div className="mt-6 grid gap-6 sm:grid-cols-2">
          <div>
            <p className="text-headline">Everyone has a part.</p>
            <p className="mt-3 text-ui text-text-muted">
              DM Sans carries the conversation, from headlines to everyday controls.
            </p>
          </div>
          <div>
            <p className="font-mono text-code">artifact: design-notes.md</p>
            <p className="mt-3 text-ui text-text-muted">
              JetBrains Mono is reserved for code and useful technical detail.
            </p>
          </div>
        </div>
      </section>
      <Separator className="my-12" />
      <section
        className="flex flex-wrap items-center justify-between gap-6"
        aria-label="Participant identities"
      >
        <div className="flex items-center gap-3">
          <Avatar kind="person" initials="JL" label="Jamie Lee" />
          <span className="text-ui font-medium">Jamie Lee</span>
        </div>
        <div className="flex items-center gap-3">
          <Avatar kind="agent" initials="SY" label="Sydney" />
          <span className="text-ui font-medium">Sydney</span>
          <Badge tone="agent">Agent</Badge>
        </div>
        <p className="text-xs text-text-muted">Illustrative identities</p>
      </section>
      <p className="mt-12 text-ui text-text-muted">swarmie · by Syntropika</p>
    </main>
  ),
};

export const SemanticPalettes: Story = {
  render: () => (
    <main className="grid gap-6 p-6 xl:grid-cols-2">
      {(["light", "dark"] as const).map((mode) => (
        <section
          key={mode}
          data-theme={mode}
          aria-label={`${mode} semantic palette`}
          className="rounded-panel bg-canvas p-6 text-ink"
        >
          <h1 className="text-title capitalize">{mode} · Dusty Blue</h1>
          <p className="mt-2 text-ui text-text-muted">
            Neutral surfaces, readable accents, and status labels on their matching soft
            backgrounds.
          </p>
          <div className="mt-6 grid grid-cols-2 gap-3 sm:grid-cols-3">
            {semanticSwatches.map(([name, background, light, dark]) => (
              <div
                key={name}
                className={cn(
                  "flex min-h-28 flex-col justify-between rounded-panel border border-transparent p-4",
                  background,
                )}
              >
                <span className="text-ui font-medium">{name}</span>
                <span className="font-mono text-xs">{mode === "light" ? light : dark}</span>
              </div>
            ))}
          </div>
          <div className="mt-6 rounded-panel bg-surface p-4">
            <p className="text-ui text-ink">Main text on a surface</p>
            <p className="mt-2 text-ui text-text-muted">Supporting text and placeholders</p>
            <a
              href={`#semantic-controls-${mode}`}
              className="mt-2 inline-block rounded-pill text-ui text-primary underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
            >
              Primary link
            </a>
          </div>
          <div className="mt-4 rounded-panel bg-inverse p-4 text-inverse-text">
            <p className="text-ui">Inverse section text</p>
            <p className="mt-2 text-ui text-inverse-muted">Supporting inverse text</p>
            <a
              href={`#semantic-controls-${mode}`}
              className="mt-2 inline-block rounded-pill text-ui text-inverse-link underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-inverse-text"
            >
              Inverse link
            </a>
          </div>
          <div id={`semantic-controls-${mode}`} className="mt-6 grid gap-3">
            <label htmlFor={`semantic-input-${mode}`} className="text-ui font-medium">
              Workspace name
            </label>
            <Input id={`semantic-input-${mode}`} placeholder="Your shared workspace" />
            <div className="flex flex-wrap gap-3">
              <Button>Primary action</Button>
              <Button variant="secondary">Secondary action</Button>
            </div>
            <div className="flex flex-wrap gap-3" aria-label="Status examples">
              <Badge tone="success">Ready</Badge>
              <Badge tone="attention">Needs attention</Badge>
              <Badge tone="danger">Unable to continue</Badge>
            </div>
          </div>
        </section>
      ))}
    </main>
  ),
};
