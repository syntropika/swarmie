import { describe, expect, test } from "bun:test";

import { renderToStaticMarkup } from "react-dom/server";

import { CheckpointHistory, OptionList, TodoList } from "./agents";
import { Sources } from "./knowledge";
import { isMapCoordinate, projectMapPlaces } from "./map-projection";
import { MessageError } from "./messages";
import { MessageTiming, QuotaBanner } from "./observability";
import { parseAnsi } from "./terminal-ansi";
import { ElicitationForm, PermissionGrant, TerminalBlock } from "./tools";

const noop = () => {};

describe("terminal rendering", () => {
  test("preserves SGR style boundaries and strips terminal control strings", () => {
    const parts = parseAnsi(
      "a\u001B[1;31mred\u001B[0m b\u001B]8;;https://example.com\u0007link\u001B]8;;\u0007\u001B[2K",
    );
    expect(parts.map((part) => part.text).join("")).toBe("ared blink");
    expect(parts.find((part) => part.text === "red")).toMatchObject({ bold: true, color: 31 });
    expect(parts.at(-1)).toMatchObject({ bold: false, color: undefined });
  });

  test("handles extended color parameters without misreading them as SGR colors", () => {
    const parts = parseAnsi("\u001B[38;2;31;32;33mtext\u001B[2mdim");
    expect(parts[0]).toMatchObject({ color: undefined, text: "text" });
    expect(parts[1]).toMatchObject({ dim: true, text: "dim" });
    expect(
      parseAnsi("before\u001B[31")
        .map((part) => part.text)
        .join(""),
    ).toBe("before");
  });

  test("collapsed terminal retains stderr semantics and exposes expansion", () => {
    const html = renderToStaticMarkup(
      <TerminalBlock
        command="check"
        statusLabel="Failed"
        exitCode={1}
        maxCollapsedLines={1}
        lines={[
          { id: "error", text: "<script>no</script>", stream: "stderr" },
          { id: "second", text: "detail" },
        ]}
      />,
    );
    expect(html).toContain("text-danger");
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain('aria-expanded="false"');
    expect(html).not.toContain(">detail<");
  });
});

describe("controlled decisions", () => {
  test("retrying replaces the error action with a polite pending status", () => {
    const html = renderToStaticMarkup(
      <MessageError
        title="Failed"
        message="Try again"
        retryLabel="Retry"
        retrying
        retryingLabel="Retrying request"
        onRetry={noop}
      />,
    );
    expect(html).toContain("Retrying request");
    expect(html).toContain('aria-live="polite"');
    expect(html).not.toContain("<button");
    expect(html).not.toContain('role="alert"');
  });
  test("accepted elicitation removes actions and locks supplied field values", () => {
    const html = renderToStaticMarkup(
      <ElicitationForm
        title="Access"
        description="Confirm"
        fields={[{ id: "enabled", label: "Enabled", kind: "checkbox", value: "true" }]}
        onChange={noop}
        submitLabel="Send"
        cancelLabel="Decline"
        onSubmit={noop}
        onCancel={noop}
        state="accepted"
        statusLabel="Sent"
      />,
    );
    expect(html).toContain('name="enabled"');
    expect(html).toContain("checked");
    expect(html).toContain("disabled");
    expect(html).toContain("Sent");
    expect(html).not.toContain('type="submit"');
  });

  test("permission receipts remove grant actions for every settled scope", () => {
    for (const state of ["session", "always", "denied"] as const) {
      const html = renderToStaticMarkup(
        <PermissionGrant
          state={state}
          statusLabel={state}
          title="Permission"
          scope={[]}
          grantLabel="Grant"
          denyLabel="Deny"
          onGrant={noop}
          onDeny={noop}
        />,
      );
      expect(html).toContain("<output");
      expect(html).not.toContain("<button");
    }
  });

  test("committed choices render only the selected receipt and lock changes", () => {
    const html = renderToStaticMarkup(
      <OptionList
        label="Choose"
        options={[
          { id: "a", label: "Alpha" },
          { id: "b", label: "Beta" },
        ]}
        selected={[]}
        choice={["b"]}
        onChange={noop}
      />,
    );
    expect(html).not.toContain("Alpha");
    expect(html).toContain("Beta");
    expect(html).toContain("disabled");
  });

  test("cancelled todos do not inflate total and current checkpoint cannot restore itself", () => {
    const todo = renderToStaticMarkup(
      <TodoList
        label="Tasks"
        items={[
          { id: "done", label: "Done", state: "done" },
          { id: "cancelled", label: "Cancelled", state: "cancelled" },
        ]}
      />,
    );
    expect(todo).toContain("1/1");
    const checkpoint = renderToStaticMarkup(
      <CheckpointHistory
        label="History"
        checkpoints={[{ id: "now", title: "Current", detail: "Saved", time: "12:00" }]}
        currentId="now"
        restoreLabel="Restore"
        onRestore={noop}
      />,
    );
    expect(checkpoint).toContain("disabled");
    expect(checkpoint).not.toContain(">Restore<");
  });
});

describe("knowledge and telemetry fidelity", () => {
  test("map adapter filters invalid geography and fits valid points within the viewport", () => {
    expect(isMapCoordinate(Number.NaN, 0)).toBe(false);
    expect(isMapCoordinate(91, 0)).toBe(false);
    const points = projectMapPlaces([
      { id: "a", latitude: 38, longitude: -9 },
      { id: "b", latitude: 39, longitude: -8 },
      { id: "bad", latitude: 100, longitude: 0 },
    ]);
    expect(points).toHaveLength(2);
    expect(points[0]).toMatchObject({ x: 48, y: 200 });
    expect(points[1]).toMatchObject({ x: 552, y: 40 });
    expect(projectMapPlaces([{ id: "one", latitude: 0, longitude: 0 }])[0]).toMatchObject({
      x: 300,
      y: 120,
    });
  });

  test("document sources remain nonlinks even if a URL is supplied", () => {
    const html = renderToStaticMarkup(
      <Sources
        label="Evidence"
        open
        sources={[
          { id: "doc", title: "Internal document", kind: "document", href: "https://example.com" },
        ]}
      />,
    );
    expect(html).toContain("Internal document");
    expect(html).not.toContain('href="https://example.com');
  });

  test("quota measures used capacity and flags ninety percent usage", () => {
    const html = renderToStaticMarkup(
      <QuotaBanner
        title="Quota"
        remaining={10}
        maximum={100}
        resetLabel="Tomorrow"
        actionLabel="Manage"
        onAction={noop}
      />,
    );
    expect(html).toContain('value="90"');
    expect(html).toContain("bg-attention-soft");
    expect(
      renderToStaticMarkup(
        <MessageTiming
          label="Timing"
          firstToken="20 ms"
          total="1 s"
          speed="10/s"
          completed={false}
        />,
      ),
    ).toBe("");
  });
});
