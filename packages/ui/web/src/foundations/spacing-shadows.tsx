import { useEffect, useRef, useState } from "react";

import { HeroUIPreview } from "../../.storybook/heroui-preview";
import { Table } from "../atoms/table";
import { Reference } from "./reference";

const steps = [
  ["1", "w-1"],
  ["2", "w-2"],
  ["3", "w-3"],
  ["4", "w-4"],
  ["6", "w-6"],
  ["8", "w-8"],
  ["12", "w-12"],
  ["16", "w-16"],
] as const;

const menuSpacing = [
  ["menu-inset", "w-menu-inset", "List boundary"],
  ["menu-row-x", "w-menu-row-x", "Horizontal row inset"],
  ["menu-row-y", "w-menu-row-y", "Vertical row inset"],
  ["menu-section-gap", "w-menu-section-gap", "Between result groups"],
  ["search-inset", "w-search-inset", "Integrated search header inset"],
] as const;

function SpacingSample({ width }: { width: string }) {
  const sample = useRef<HTMLDivElement>(null);
  const [value, setValue] = useState("");
  useEffect(() => {
    if (sample.current) {
      setValue(getComputedStyle(sample.current).width);
    }
  }, []);
  return (
    <div className="grid gap-2">
      <div ref={sample} className={`h-3 rounded-full bg-accent ${width}`} />
      <code className="text-xs">{value}</code>
    </div>
  );
}
export function SpacingReference({ theme }: { theme: "light" | "dark" }) {
  return (
    <Reference
      title="Spacing"
      intro="Use the same scale for gaps, padding and margins across web and desktop. Choose spacing by the relationship between elements."
    >
      <HeroUIPreview theme={theme}>
        <Table>
          <Table.ScrollContainer>
            <Table.Content aria-label="Spacing scale">
              <Table.Header>
                <Table.Column isRowHeader>Step</Table.Column>
                <Table.Column>Use</Table.Column>
                <Table.Column>Rendered distance</Table.Column>
              </Table.Header>
              <Table.Body>
                {steps.map(([step, width]) => (
                  <Table.Row id={step} key={step}>
                    <Table.Cell>{step}</Table.Cell>
                    <Table.Cell>
                      <code>
                        gap-{step} · p-{step} · m-{step}
                      </code>
                    </Table.Cell>
                    <Table.Cell>
                      <SpacingSample width={width} />
                    </Table.Cell>
                  </Table.Row>
                ))}
              </Table.Body>
            </Table.Content>
          </Table.ScrollContainer>
        </Table>
        <section className="grid gap-3">
          <h2 className="font-semibold">Menu spacing roles</h2>
          <p className="text-sm text-muted">
            SearchDialog consumes these semantic roles. General dialogs keep their 16px content
            inset; search separates the field, list and rows into their own zones.
          </p>
          <Table>
            <Table.ScrollContainer>
              <Table.Content aria-label="Menu spacing roles">
                <Table.Header>
                  <Table.Column isRowHeader>Token</Table.Column>
                  <Table.Column>Relationship</Table.Column>
                  <Table.Column>Rendered distance</Table.Column>
                </Table.Header>
                <Table.Body>
                  {menuSpacing.map(([token, width, use]) => (
                    <Table.Row id={token} key={token}>
                      <Table.Cell>
                        <code>--spacing-{token}</code>
                      </Table.Cell>
                      <Table.Cell>{use}</Table.Cell>
                      <Table.Cell>
                        <SpacingSample width={width} />
                      </Table.Cell>
                    </Table.Row>
                  ))}
                </Table.Body>
              </Table.Content>
            </Table.ScrollContainer>
          </Table>
        </section>
        <div className="grid gap-6 @lg:grid-cols-3">
          <div className="grid gap-2 rounded-2xl bg-surface p-4">
            <h2 className="font-semibold">Related content · gap-2</h2>
            <p className="text-sm text-muted">A label and its explanation stay close.</p>
          </div>
          <div className="grid gap-4 rounded-2xl bg-surface p-4">
            <h2 className="font-semibold">A content group · gap-4</h2>
            <p className="text-sm text-muted">Separate related rows or controls.</p>
          </div>
          <div className="grid gap-8 rounded-2xl bg-surface p-4">
            <h2 className="font-semibold">A new section · gap-8</h2>
            <p className="text-sm text-muted">More distance communicates a new group.</p>
          </div>
        </div>
      </HeroUIPreview>
    </Reference>
  );
}
function ShadowSample({
  label,
  classes,
  theme,
  description,
}: {
  label: string;
  classes: string;
  theme: "light" | "dark";
  description: string;
}) {
  const sample = useRef<HTMLDivElement>(null);
  const [value, setValue] = useState("");
  useEffect(() => {
    if (sample.current?.closest<HTMLElement>("[data-theme]")?.dataset.theme === theme) {
      setValue(getComputedStyle(sample.current).boxShadow);
    }
  }, [theme]);
  return (
    <section className="grid min-w-0 gap-3">
      <div ref={sample} className={`grid min-h-28 place-content-center rounded-2xl p-6 ${classes}`}>
        <h2 className="font-semibold">{label}</h2>
      </div>
      <p className="text-sm text-muted">{description}</p>
      <code className="text-xs break-words">{value}</code>
    </section>
  );
}
export function ShadowsReference({ theme }: { theme: "light" | "dark" }) {
  return (
    <Reference
      title="Shadows"
      intro="Use elevation to explain which surface sits above another. Fields, persistent surfaces and floating overlays keep their own shared shadow role."
    >
      <HeroUIPreview theme={theme}>
        <div className="grid gap-8 @lg:grid-cols-3">
          <ShadowSample
            theme={theme}
            label="Field"
            classes="bg-field text-field-foreground shadow-field"
            description="The field shadow supports the control’s boundary."
          />
          <ShadowSample
            theme={theme}
            label="Surface"
            classes="bg-surface text-surface-foreground shadow-surface"
            description="A persistent component uses the surface shadow."
          />
          <ShadowSample
            theme={theme}
            label="Overlay"
            classes="bg-overlay text-overlay-foreground shadow-overlay"
            description="Floating menus and dialogs use the overlay shadow."
          />
        </div>
        <p className="text-sm text-muted">
          The values above are read from the rendered samples. Choose the semantic shadow role
          rather than stacking extra shadows on individual controls.
        </p>
      </HeroUIPreview>
    </Reference>
  );
}
