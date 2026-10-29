import { expect, test } from "bun:test";

import type { Tab } from "@swarmie/plugin-views-browser";
import { Tabs } from "@swarmie/ui-web";
import { renderToStaticMarkup } from "react-dom/server";

import { ViewTabList } from "../src/views/view-tabs";

const tabs: Tab[] = [
  { id: "first", title: "First view", kind: "start" },
  { id: "second", title: "Second view", kind: "notes" },
  { id: "third", title: "Third view", kind: "state" },
];

function markup(selected: string) {
  return renderToStaticMarkup(
    <Tabs selectedKey={selected}>
      <header>
        <ViewTabList tabs={tabs} selected={selected} close={() => {}} />
      </header>
      <main>
        <Tabs.Panel id={selected}>Selected content</Tabs.Panel>
      </main>
    </Tabs>,
  );
}

test("header view tabs preserve selected state and associate with the content panel", () => {
  const html = markup("second");
  const tabElements = [...html.matchAll(/<div[^>]*role="tab"[^>]*>/gu)].map(([element]) => element);
  expect(tabElements).toHaveLength(3);
  expect(tabElements.filter((element) => element.includes('aria-selected="true"'))).toHaveLength(1);
  const selected = tabElements.find((element) => element.includes('aria-selected="true"'));
  expect(selected).toContain('aria-label="Second view"');
  const selectedId = selected?.match(/ id="(?<id>[^"]+)"/u)?.groups?.id;
  const panelId = selected?.match(/ aria-controls="(?<id>[^"]+)"/u)?.groups?.id;
  expect(selectedId).toBeTruthy();
  expect(panelId).toBeTruthy();
  expect(html).toContain(`id="${panelId}"`);
  expect(html).toContain(`aria-labelledby="${selectedId}"`);
  expect(html).toContain('role="tabpanel"');
});

test("only the active close action enters the tab order and remains outside its tab", () => {
  const html = markup("third");
  const closeButtons = [...html.matchAll(/<button[^>]*aria-label="Close [^"]+"[^>]*>/gu)].map(
    ([element]) => element,
  );
  expect(closeButtons).toHaveLength(3);
  expect(closeButtons.filter((element) => element.includes('tabindex="0"'))).toHaveLength(1);
  expect(closeButtons.find((element) => element.includes('tabindex="0"'))).toContain(
    'aria-label="Close Third view"',
  );
  let nestedButtons = 0;
  new HTMLRewriter()
    .on('[role="tab"] button', {
      element() {
        nestedButtons += 1;
      },
    })
    .transform(html);
  expect(nestedButtons).toBe(0);
});
