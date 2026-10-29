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
      [
        "Primary",
        "action-primary / action-primary-foreground",
        "bg-action-primary text-action-primary-foreground",
      ],
      ["Secondary", "default / default-foreground", "bg-default text-default-foreground"],
      ["Accent", "accent / accent-foreground", "bg-accent text-accent-foreground"],
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
      ["Destructive action", "danger / danger-foreground", "bg-danger text-danger-foreground"],
      [
        "Soft danger",
        "danger-soft / danger-soft-foreground",
        "bg-danger-soft text-danger-soft-foreground",
      ],
      ["Validation error", "danger", "bg-background text-danger"],
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
      <div ref={sample} className={`flex h-12 items-center rounded-3xl px-4 font-medium ${row[2]}`}>
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
      intro="HeroUI semantic colors with the Swarmie coral accent. Primary buttons use the inverse neutral action pair. The vendor palette supplies the other component colors in each theme."
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
          <h2 className="text-xl font-semibold">Stock component colors</h2>
          <p className="max-w-3xl text-sm text-muted">
            The card, avatar, chip, field and actions use the public HeroUI contracts. Their
            variants keep the vendor colors, dimensions and interaction states.
          </p>
          <Card className="w-full max-w-lg">
            <Card.Header>
              <div className="flex items-center gap-3">
                <Avatar aria-label="Design team">
                  <Avatar.Fallback>DT</Avatar.Fallback>
                </Avatar>
                <div className="flex flex-wrap items-center gap-3">
                  <Card.Title>Website refresh</Card.Title>
                  <Chip color="accent" variant="soft">
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
        </section>
        <p className="text-sm text-muted">
          Destructive actions and validation use the vendor danger roles. Status also needs a label
          or icon. Use semantic roles consistently across web and desktop.
        </p>
      </HeroUIPreview>
    </Reference>
  );
}
