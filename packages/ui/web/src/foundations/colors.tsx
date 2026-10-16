import { useEffect, useRef, useState } from "react";

import { HeroUIPreview } from "../../.storybook/heroui-preview";
import { Avatar } from "../atoms/avatar";
import { Button } from "../atoms/button";
import { Card } from "../atoms/card";
import { Chip } from "../atoms/chip";
import { Input } from "../atoms/input";
import { Table } from "../atoms/table";
import { TextField, Label } from "../atoms/text-field";
import { Reference } from "./reference";

const groups = [
  {
    title: "Surfaces and text",
    rows: [
      ["Base canvas", "background / foreground", "bg-background text-foreground"],
      ["Component surface", "surface / surface-foreground", "bg-surface text-surface-foreground"],
      [
        "Secondary surface",
        "surface-secondary / surface-secondary-foreground",
        "bg-surface-secondary text-surface-secondary-foreground",
      ],
      ["Floating content", "overlay / overlay-foreground", "bg-overlay text-overlay-foreground"],
      ["Field", "field-background / field-foreground", "bg-field text-field-foreground"],
      ["Supporting text", "muted", "bg-background text-muted"],
    ],
  },
  {
    title: "Actions",
    rows: [
      ["Primary", "accent / accent-foreground", "bg-accent text-accent-foreground"],
      ["Secondary", "default / default-foreground", "bg-default text-default-foreground"],
    ],
  },
  {
    title: "Status",
    rows: [
      ["Success", "success / success-foreground", "bg-success text-success-foreground"],
      [
        "Soft success",
        "success-soft / success-soft-foreground",
        "bg-success-soft text-success-soft-foreground",
      ],
      ["Warning", "warning / warning-foreground", "bg-warning text-warning-foreground"],
      [
        "Destructive action",
        "danger-solid / danger-solid-foreground",
        "bg-danger-solid text-danger-solid-foreground",
      ],
      [
        "Soft danger",
        "danger-soft / danger-soft-foreground",
        "bg-danger-soft text-danger-soft-foreground",
      ],
      ["Validation error", "danger", "bg-background text-danger"],
    ],
  },
  {
    title: "Brand accents",
    rows: [
      ["Mint", "mint", "bg-mint text-foreground"],
      ["Lilac", "lilac", "bg-lilac text-foreground"],
      ["Peach", "peach", "bg-peach text-foreground"],
      ["Mustard", "butter", "bg-butter text-foreground"],
    ],
  },
] as const;

function ColorSample({
  row,
  theme,
}: {
  row: readonly [string, string, string];
  theme: "light" | "dark";
}) {
  const sample = useRef<HTMLDivElement>(null);
  const [value, setValue] = useState("");
  useEffect(() => {
    const element = sample.current;
    if (!element || element.closest<HTMLElement>("[data-theme]")?.dataset.theme !== theme) {
      return;
    }
    const style = getComputedStyle(element);
    setValue(`${style.backgroundColor} / ${style.color}`);
  }, [theme]);
  return (
    <div className="grid gap-2">
      <div ref={sample} className={`flex h-12 items-center rounded-lg px-4 font-medium ${row[2]}`}>
        Aa
      </div>
      <code className="text-xs break-words">{value}</code>
    </div>
  );
}

export function ColorReference({ theme }: { theme: "light" | "dark" }) {
  return (
    <Reference
      title="Colors"
      intro="Swarmie’s semantic palette. Choose colors by their role and keep each background paired with its foreground. The same roles adapt to light and dark themes."
    >
      <HeroUIPreview theme={theme}>
        {groups.map((group) => (
          <section key={group.title} className="grid gap-4">
            <h2 className="text-xl font-semibold">{group.title}</h2>
            <Table>
              <Table.ScrollContainer>
                <Table.Content aria-label="colors reference" className="table-fixed">
                  <Table.Header>
                    <Table.Column isRowHeader>Role and tokens</Table.Column>
                    <Table.Column>Sample and current CSS values</Table.Column>
                  </Table.Header>
                  <Table.Body>
                    {group.rows.map((row) => (
                      <Table.Row key={row[0]} id={row[0]}>
                        <Table.Cell>
                          <p className="font-medium">{row[0]}</p>
                          <code className="text-xs break-words text-muted">{row[1]}</code>
                        </Table.Cell>
                        <Table.Cell>
                          <ColorSample row={row} theme={theme} />
                        </Table.Cell>
                      </Table.Row>
                    ))}
                  </Table.Body>
                </Table.Content>
              </Table.ScrollContainer>
              <Table.Footer>Rendered background / foreground values in the theme.</Table.Footer>
            </Table>
          </section>
        ))}
        <section className="grid gap-4">
          <h2 className="text-xl font-semibold">Category accents on neutral foundations</h2>
          <p className="text-sm text-muted">
            Keep the canvas, surfaces, fields and actions neutral. Use mint, lilac, peach and
            mustard for named categories, project markers, avatar initials and illustration details.
            These examples do not assign a mandatory meaning to a hue; keep each assignment stable
            within its view and include a readable label.
          </p>
          <div className="flex flex-wrap gap-3">
            <Chip color="mint">Research</Chip>
            <Chip color="lilac">Design</Chip>
            <Chip color="peach">Community</Chip>
            <Chip color="mustard">Ideas</Chip>
          </div>
        </section>
        <section className="grid gap-4">
          <h2 className="text-xl font-semibold">A neutral card with a category accent</h2>
          <p className="max-w-3xl text-sm text-muted">
            The lilac initials and mint label identify content. The card, field and actions retain
            their shared neutral roles, vendor dimensions and rounded shapes in either theme.
          </p>
          <Card className="w-full max-w-lg">
            <Card.Header>
              <div className="flex items-center gap-3">
                <Avatar aria-label="Design team">
                  <Avatar.Fallback className="bg-lilac text-foreground">DT</Avatar.Fallback>
                </Avatar>
                <div className="flex flex-wrap items-center gap-3">
                  <Card.Title>Website refresh</Card.Title>
                  <Chip color="mint" variant="soft">
                    Design project
                  </Chip>
                </div>
              </div>
            </Card.Header>
            <Card.Content>
              <TextField defaultValue="Website refresh">
                <Label>Project name</Label>
                <Input />
              </TextField>
            </Card.Content>
            <Card.Footer className="flex flex-wrap gap-3">
              <Button>Save changes</Button>
              <Button variant="secondary">Cancel</Button>
            </Card.Footer>
          </Card>
          <Table>
            <Table.ScrollContainer>
              <Table.Content aria-label="Category color usage">
                <Table.Header>
                  <Table.Column isRowHeader>Use</Table.Column>
                  <Table.Column>Keep neutral or semantic</Table.Column>
                </Table.Header>
                <Table.Body>
                  <Table.Row id="categories">
                    <Table.Cell>Named chips, groups and project labels</Table.Cell>
                    <Table.Cell>Primary actions, focus and selection</Table.Cell>
                  </Table.Row>
                  <Table.Row id="avatars">
                    <Table.Cell>Avatar fallback fills with visible initials</Table.Cell>
                    <Table.Cell>Presence and operational status</Table.Cell>
                  </Table.Row>
                  <Table.Row id="illustrations">
                    <Table.Cell>Illustration details and labeled diagram groups</Table.Cell>
                    <Table.Cell>Default canvas, form fields and card depth</Table.Cell>
                  </Table.Row>
                  <Table.Row id="status">
                    <Table.Cell>Category labels with text or icons</Table.Cell>
                    <Table.Cell>Success, warning, danger and permissions</Table.Cell>
                  </Table.Row>
                </Table.Body>
              </Table.Content>
            </Table.ScrollContainer>
          </Table>
        </section>
        <p className="text-sm text-muted">
          A destructive button uses the solid red pair; validation uses readable red text. Status
          also needs a label or icon. Use semantic roles consistently across web and desktop.
        </p>
      </HeroUIPreview>
    </Reference>
  );
}
