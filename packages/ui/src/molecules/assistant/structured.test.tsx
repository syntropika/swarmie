import { expect, test } from "bun:test";

import { renderToStaticMarkup } from "react-dom/server";

import {
  ActivityGraph,
  AssistantChart,
  DataTable,
  FlowGraph,
  JobProgress,
  ScoreBreakdown,
} from "./structured";
import { TableValue } from "./table-value";

test("terminal jobs expose outcome and elapsed receipt without a cancel action", () => {
  const html = renderToStaticMarkup(
    <JobProgress
      label="Export"
      eta="Complete"
      cancelLabel="Cancel export"
      stages={[{ id: "work", label: "Work", weight: 1, progress: 100 }]}
      onCancel={() => {}}
      outcome={{ state: "completed", label: "Complete", summary: "Export saved" }}
      elapsed="12 seconds"
    />,
  );
  expect(html).toContain("Export saved");
  expect(html).toContain("12 seconds");
  expect(html).not.toContain("Cancel export");
});

test("chart SVG and equivalent table both retain losses", () => {
  const html = renderToStaticMarkup(
    <AssistantChart
      label="Profit"
      labels={["Loss", "Gain"]}
      kind="bar"
      series={[{ id: "profit", label: "Profit", values: [-10, 20] }]}
    />,
  );
  expect(Number(/<rect[^>]+height="(?<height>[^"]+)"/u.exec(html)?.groups?.height)).toBeCloseTo(
    53.33333333333333,
  );
  expect(html).toContain('x="170"');
  expect(html).toContain(">-10</td>");
});

test("graph execution states and score aggregate remain visible", () => {
  const html = renderToStaticMarkup(
    <FlowGraph
      label="Flow"
      edges={[]}
      nodes={[
        { id: "a", label: "Inspect", x: 10, y: 10, state: "completed", statusLabel: "Complete" },
        { id: "b", label: "Apply", x: 200, y: 10, state: "running", statusLabel: "Running" },
      ]}
      onSelect={() => {}}
    />,
  );
  expect(html).toContain("stroke-success");
  expect(html).toContain("stroke-primary");
  const score = renderToStaticMarkup(
    <ScoreBreakdown
      label="Review"
      criteria={[
        {
          id: "one",
          label: "Coverage",
          score: 7,
          maximum: 10,
          weight: 100,
          explanation: "Visible detail",
        },
      ]}
      total={7}
      outOf={10}
      verdict="Pass"
      visibleCount={0}
    />,
  );
  expect(score).toContain("Pass");
  expect(score).not.toContain("Visible detail");
});

test("calendar labels align to real dates and supplied sorting overrides local defaults", () => {
  const calendar = renderToStaticMarkup(
    <ActivityGraph
      label="Activity"
      legend="Less to more"
      days={[{ date: "2026-10-01", value: 2, label: "October 1: 2 events" }]}
    />,
  );
  expect(calendar).toContain("Thu");
  expect(calendar).toContain("2026-10-01");
  expect(calendar.match(/data-placeholder="true"/gu)?.length).toBe(3);
  const table = renderToStaticMarkup(
    <DataTable
      caption="Items"
      emptyLabel="Empty"
      rows={[{ id: "b" }, { id: "a" }]}
      rowId={(row) => row.id}
      columns={[
        {
          id: "id",
          label: "ID",
          render: (row) => row.id,
          compare: (a, b) => a.id.localeCompare(b.id),
        },
      ]}
      sort={{ id: "id", direction: "ascending" }}
    />,
  );
  expect(table).toContain('aria-sort="ascending"');
  expect(table.indexOf(">a</td>")).toBeLessThan(table.indexOf(">b</td>"));
});

test("table formatters accept controlled time and locale", () => {
  expect(
    renderToStaticMarkup(
      <TableValue
        value={1000}
        format={{ kind: "number", options: { style: "currency", currency: "USD" } }}
      />,
    ),
  ).toContain("$1,000.00");
  expect(
    renderToStaticMarkup(
      <TableValue
        value={new Date("2026-10-01T00:00:00Z")}
        format={{ kind: "relative" }}
        relativeTo={Date.parse("2026-10-02T00:00:00Z")}
      />,
    ),
  ).toContain("yesterday");
});
