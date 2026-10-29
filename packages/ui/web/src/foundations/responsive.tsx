// oxlint-disable shadcn/no-raw-colors -- HeroUI token utilities are compiled by the isolated catalog stylesheet.
import { useState } from "react";

import { HeroUIPreview } from "../../.storybook/heroui-preview";
import { Button } from "../atoms/button";
import { Table } from "../atoms/table";
import { Reference } from "./reference";

const widthNames = ["Narrow", "Medium", "Wide"] as const;
const widths = { Narrow: "w-56", Medium: "w-96", Wide: "w-full" } as const;

export function ResponsiveReference({ theme }: { theme: "light" | "dark" }) {
  const [width, setWidth] = useState<keyof typeof widths>("Wide");
  return (
    <Reference
      title="Responsive design"
      intro="Start with the narrow layout, then add space and columns when the content can use them. Reusable components follow their container; page shells can follow the viewport."
    >
      <HeroUIPreview theme={theme}>
        <section className="grid gap-4">
          <h2 className="text-xl font-semibold">Same component, different available width</h2>
          <fieldset aria-label="Example container width" className="flex flex-wrap gap-2">
            {widthNames.map((name) => (
              <Button
                key={name}
                variant={width === name ? "primary" : "secondary"}
                aria-pressed={width === name}
                onPress={() => {
                  setWidth(name);
                }}
              >
                {name}
              </Button>
            ))}
          </fieldset>
          <div className={`@container max-w-full rounded-3xl bg-surface p-4 ${widths[width]}`}>
            <div className="grid gap-4 @lg:grid-cols-2">
              <div className="bg-surface-secondary rounded-3xl p-4">
                <h3 className="font-semibold">Review changes</h3>
                <p className="mt-2 text-sm text-muted">
                  The main content remains readable in a narrow panel.
                </p>
              </div>
              <div className="bg-surface-secondary rounded-3xl p-4">
                <h3 className="font-semibold">Supporting detail</h3>
                <p className="mt-2 text-sm text-muted">
                  At a 32rem container width, the content uses two columns.
                </p>
              </div>
            </div>
          </div>
        </section>
        <section className="grid gap-4">
          <h2 className="text-xl font-semibold">Choose the right boundary</h2>
          <Table>
            <Table.ScrollContainer>
              <Table.Content aria-label="responsive reference">
                <Table.Header>
                  <Table.Column isRowHeader>Situation</Table.Column>
                  <Table.Column>Use</Table.Column>
                  <Table.Column>Example</Table.Column>
                </Table.Header>
                <Table.Body>
                  <Table.Row>
                    <Table.Cell>Card, form or reusable panel</Table.Cell>
                    <Table.Cell>Container query</Table.Cell>
                    <Table.Cell>
                      <code>@container → @lg:grid-cols-2</code>
                    </Table.Cell>
                  </Table.Row>
                  <Table.Row>
                    <Table.Cell>Page shell or screen navigation</Table.Cell>
                    <Table.Cell>Viewport breakpoint</Table.Cell>
                    <Table.Cell>
                      <code>md: / lg:</code>
                    </Table.Cell>
                  </Table.Row>
                  <Table.Row>
                    <Table.Cell>Reading content</Table.Cell>
                    <Table.Cell>Flexible width with a maximum</Table.Cell>
                    <Table.Cell>
                      <code>w-full max-w-3xl</code>
                    </Table.Cell>
                  </Table.Row>
                </Table.Body>
              </Table.Content>
            </Table.ScrollContainer>
          </Table>
        </section>
        <p className="text-sm text-muted">
          Keep labels visible, let text and actions wrap, preserve focus order, and test the same
          component in both narrow and wide placements.
        </p>
      </HeroUIPreview>
    </Reference>
  );
}
