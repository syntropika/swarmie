import { expect, test } from "bun:test";

import { Pin } from "lucide-react";
import { renderToStaticMarkup } from "react-dom/server";

import { ChoiceList, IconButton, Item, Menu } from "../src/controls";

test("choice triggers retain the visible current value and identify their purpose", () => {
  for (const [text, label] of [
    ["No project", "Select project"],
    ["This computer", "Select execution node"],
    ["Queue", "Message delivery"],
    ["Example model · Medium", "Select provider, model and thinking"],
  ] as const) {
    const markup = renderToStaticMarkup(
      <Menu label={label} text={text} size="composerCap">
        {() => <Item onClick={() => undefined}>Configure</Item>}
      </Menu>,
    );
    expect(markup).toContain(`aria-label="${text}: ${label}"`);
    expect(markup).toContain(`title="${text}: ${label}"`);
    expect(markup).toContain(`>${text}</span>`);
  }
});

test("visual active state does not imply toggle meaning", () => {
  const current = renderToStaticMarkup(
    <IconButton icon={Pin} label="Conversations" active aria-current="page" />,
  );
  const toggle = renderToStaticMarkup(
    <IconButton icon={Pin} label="Pin conversation" active aria-pressed />,
  );
  const disclosure = renderToStaticMarkup(
    <IconButton icon={Pin} label="Navigation" active aria-expanded />,
  );
  expect(current).toContain('aria-current="page"');
  expect(current).not.toContain("aria-pressed");
  expect(toggle).toContain('aria-pressed="true"');
  expect(disclosure).toContain('aria-expanded="true"');
  expect(disclosure).not.toContain("aria-pressed");
});

test("choice lists expose the selected option separately from plain actions", () => {
  for (const selected of ["Low", "High"] as const) {
    const markup = renderToStaticMarkup(
      <>
        <ChoiceList
          label="Thinking"
          value={selected}
          items={[
            { id: "Low", label: "Low" },
            { id: "High", label: "High" },
          ]}
          onAction={() => undefined}
        />
        <Item onClick={() => undefined}>Configure providers</Item>
      </>,
    );
    const selectedLabels: (string | null)[] = [];
    new HTMLRewriter()
      .on('[role="option"][aria-selected="true"]', {
        element(element) {
          selectedLabels.push(element.getAttribute("aria-label"));
        },
      })
      .transform(markup);
    expect(selectedLabels).toEqual([selected]);
    expect(markup).toContain('role="listbox"');
    expect(markup).toContain('aria-label="Thinking"');
    expect(markup).toContain('aria-selected="false"');
    expect(markup).toContain('type="button"');
    expect(markup).not.toContain('aria-pressed="true"');
  }
});
