import { Button, Input, Label, TextField } from "@heroui/react";
import { useEffect, useRef, useState } from "react";

import { HeroUIPreview } from "../../.storybook/heroui-preview";
import { Icon, iconNames } from "../atoms/icon";
import { Table } from "../atoms/table";
import { Reference } from "./reference";

function MotionExamples({ theme }: { theme: "light" | "dark" }) {
  const action = useRef<HTMLButtonElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const [count, setCount] = useState(0);
  const [rows, setRows] = useState<
    { name: string; properties: string; duration: string; easing: string }[]
  >([]);
  useEffect(() => {
    const update = () => {
      if (action.current?.closest<HTMLElement>("[data-theme]")?.dataset.theme !== theme) {
        return;
      }
      setRows(
        (
          [
            ["Button", action.current],
            ["Input", field.current],
          ] as const
        ).flatMap(([name, element]) => {
          if (!element) {
            return [];
          }
          const css = getComputedStyle(element);
          return [
            {
              name,
              properties: css.transitionProperty,
              duration: css.transitionDuration,
              easing: css.transitionTimingFunction,
            },
          ];
        }),
      );
    };
    update();
    const media = matchMedia("(prefers-reduced-motion: reduce)");
    media.addEventListener("change", update);
    return () => {
      media.removeEventListener("change", update);
    };
  }, [theme]);
  return (
    <>
      <section className="grid gap-4">
        <h2 className="text-xl font-semibold">Feedback that belongs to the control</h2>
        <div className="flex flex-wrap gap-3">
          <Button
            ref={action}
            onPress={() => {
              setCount((value) => value + 1);
            }}
          >
            Try the press feedback
          </Button>
          <Button variant="secondary">Hover and focus</Button>
        </div>
        <output className="text-sm text-muted">Actions completed: {count}</output>
        <TextField>
          <Label>Focus and hover feedback</Label>
          <Input ref={field} placeholder="Move between controls with Tab" />
        </TextField>
      </section>
      <Table>
        <Table.ScrollContainer>
          <Table.Content aria-label="behavior reference">
            <Table.Header>
              <Table.Column isRowHeader>Control</Table.Column>
              <Table.Column>Properties</Table.Column>
              <Table.Column>Duration</Table.Column>
              <Table.Column>Easing</Table.Column>
            </Table.Header>
            <Table.Body>
              {rows.map((row) => (
                <Table.Row key={row.name}>
                  <Table.Cell>{row.name}</Table.Cell>
                  <Table.Cell className="break-words">
                    <code>{row.properties}</code>
                  </Table.Cell>
                  <Table.Cell>
                    <code>{row.duration}</code>
                  </Table.Cell>
                  <Table.Cell className="break-words">
                    <code>{row.easing}</code>
                  </Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table.Content>
        </Table.ScrollContainer>
        <Table.Footer>
          Transitions measured from the rendered vendor controls, including the current
          reduced-motion setting.
        </Table.Footer>
      </Table>
    </>
  );
}

export function MotionReference({ theme }: { theme: "light" | "dark" }) {
  return (
    <Reference
      title="Motion"
      intro="Use movement to acknowledge an action, explain a transition or preserve context. Keep repeated reading still and respect the operating system’s reduced-motion preference."
    >
      <HeroUIPreview theme={theme}>
        <MotionExamples theme={theme} />
        <p className="text-sm text-muted">
          Use motion-safe for optional animation. Reduced motion must preserve the result, focus and
          loading feedback.
        </p>
      </HeroUIPreview>
    </Reference>
  );
}

export function IconsReference({ theme }: { theme: "light" | "dark" }) {
  return (
    <Reference
      title="Icons"
      intro="Use the shared named icon entry, keep stroke and size consistent, and give every icon-only action an accessible name. The gallery lists the icons currently exposed by the shared web package."
    >
      <HeroUIPreview theme={theme}>
        <section className="grid gap-4">
          <h2 className="text-xl font-semibold">Size and accessibility</h2>
          <Table>
            <Table.ScrollContainer>
              <Table.Content aria-label="behavior reference">
                <Table.Header>
                  <Table.Column isRowHeader>Role</Table.Column>
                  <Table.Column>Size</Table.Column>
                  <Table.Column>Example</Table.Column>
                </Table.Header>
                <Table.Body>
                  <Table.Row>
                    <Table.Cell>Compact supporting icon</Table.Cell>
                    <Table.Cell>16px · size-4</Table.Cell>
                    <Table.Cell>
                      <Icon name="search" className="size-4" />
                    </Table.Cell>
                  </Table.Row>
                  <Table.Row>
                    <Table.Cell>Standard action</Table.Cell>
                    <Table.Cell>20px · size-5</Table.Cell>
                    <Table.Cell>
                      <Icon name="settings" className="size-5" />
                    </Table.Cell>
                  </Table.Row>
                  <Table.Row>
                    <Table.Cell>Prominent action</Table.Cell>
                    <Table.Cell>24px · size-6</Table.Cell>
                    <Table.Cell>
                      <Icon name="plus" className="size-6" />
                    </Table.Cell>
                  </Table.Row>
                </Table.Body>
              </Table.Content>
            </Table.ScrollContainer>
          </Table>
          <div className="flex flex-wrap gap-3">
            <Button aria-label="Search">
              <Icon name="search" className="size-5" />
            </Button>
            <Button variant="secondary">
              <Icon name="plus" className="size-5" />
              New project
            </Button>
          </div>
          <p className="text-sm text-muted">
            Decorative icons are hidden from assistive technology. An icon-only button names its
            action; an icon beside text uses that visible label. The shared web entry uses Lucide;
            application-owned illustrations may also supply their own SVG artwork.
          </p>
        </section>
        <section className="grid gap-4">
          <h2 className="text-xl font-semibold">Available icons</h2>
          <Table>
            <Table.ScrollContainer>
              <Table.Content aria-label="behavior reference">
                <Table.Header>
                  <Table.Column isRowHeader>Shared name</Table.Column>
                  <Table.Column>Icon</Table.Column>
                </Table.Header>
                <Table.Body>
                  {iconNames.map((name) => (
                    <Table.Row key={name}>
                      <Table.Cell>
                        <code>{name}</code>
                      </Table.Cell>
                      <Table.Cell>
                        <Icon name={name} className="size-5" />
                      </Table.Cell>
                    </Table.Row>
                  ))}
                </Table.Body>
              </Table.Content>
            </Table.ScrollContainer>
          </Table>
        </section>
      </HeroUIPreview>
    </Reference>
  );
}
