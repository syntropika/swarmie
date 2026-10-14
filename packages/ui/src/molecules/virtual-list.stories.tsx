import type { Meta, StoryObj } from "@storybook/react-vite";
import { useCallback, useRef, useState } from "react";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { Button } from "../atoms/button";
import { VirtualList } from "./virtual-list";
import type { VirtualListHandle, VirtualListPosition } from "./virtual-list";

type Entry = { key: string; number: number; paragraphs: number };

function createEntries(first: number, count: number): Entry[] {
  return Array.from({ length: count }, (_, index) => ({
    key: `row-${first + index}`,
    number: first + index,
    paragraphs: 1 + (index % 4),
  }));
}

function getEntryKey(entry: Entry) {
  return entry.key;
}

function renderEntry(entry: Entry) {
  return (
    <article className="border-b border-border px-4 py-3">
      <h3 className="text-ui font-medium text-ink">Entry {entry.number}</h3>
      <p className="mt-2 text-ui leading-relaxed text-text-muted">
        {"Variable content keeps this row measured as its text changes. ".repeat(entry.paragraphs)}
      </p>
    </article>
  );
}

function HistoryExample({
  initialPosition,
  keepHeader = false,
  emptyInitially = false,
}: {
  initialPosition?: VirtualListPosition;
  keepHeader?: boolean;
  emptyInitially?: boolean;
}) {
  const [entries, setEntries] = useState(() => (emptyInitially ? [] : createEntries(100, 1000)));
  const [mounted, setMounted] = useState(true);
  const [compact, setCompact] = useState(false);
  const [savedPosition, setSavedPosition] = useState(initialPosition);
  const latestPosition = useRef(initialPosition);
  const list = useRef<VirtualListHandle>(null);
  const handlePositionChange = useCallback((position: VirtualListPosition) => {
    latestPosition.current = position;
  }, []);

  return (
    <div className="flex max-w-2xl flex-col gap-3">
      <div className="flex flex-wrap gap-2">
        <Button variant="secondary" onClick={() => list.current?.scrollToKey("row-500")}>
          Jump to entry 500
        </Button>
        <Button
          variant="secondary"
          onClick={() => list.current?.scrollToKey("row-500", { focus: false })}
        >
          Jump and retain focus
        </Button>
        <Button variant="secondary" onClick={() => list.current?.scrollToEnd()}>
          Jump to latest
        </Button>
        <Button
          variant="secondary"
          onClick={() => {
            setCompact((current) => !current);
          }}
        >
          {compact ? "Expand viewport" : "Compact viewport"}
        </Button>
        <Button
          variant="secondary"
          onClick={() => {
            setEntries((current) => {
              const number = (current.at(-1)?.number ?? 99) + 1;
              return [...current, { key: `row-${number}`, number, paragraphs: 20 }];
            });
          }}
        >
          Append new entry
        </Button>
        <Button
          variant="secondary"
          disabled={entries.length > 1000}
          onClick={() => {
            setEntries((current) => [...createEntries(0, 100), ...current]);
          }}
        >
          Prepend 100 entries
        </Button>
        <Button
          variant="secondary"
          onClick={() => {
            setEntries((current) =>
              current.map((entry, index) =>
                index === current.length - 1
                  ? { ...entry, paragraphs: entry.paragraphs + 8 }
                  : entry,
              ),
            );
          }}
        >
          Grow latest entry
        </Button>
        <Button
          variant="secondary"
          onClick={() => {
            if (mounted) {
              setSavedPosition(latestPosition.current);
            }
            setMounted((current) => !current);
          }}
        >
          {mounted ? "Leave list" : "Return to list"}
        </Button>
      </div>
      {mounted ? (
        <VirtualList
          ref={list}
          items={entries}
          getItemKey={getEntryKey}
          renderItem={renderEntry}
          label="Example history"
          className={
            compact
              ? "h-32 w-72 rounded-panel border border-transparent"
              : "h-96 w-full rounded-panel border border-transparent"
          }
          initialPosition={savedPosition}
          onPositionChange={handlePositionChange}
          followEnd
          header={
            keepHeader || entries.length <= 1000 ? (
              <p className="px-4 py-3 text-ui text-text-muted">
                Earlier entries appear above the current reading position.
              </p>
            ) : undefined
          }
        />
      ) : (
        <p className="text-ui text-text-muted">
          The list is unmounted. Its stable reading anchor is retained by the consumer.
        </p>
      )}
    </div>
  );
}

function getRow(canvasElement: HTMLElement, key: string) {
  const row = canvasElement.querySelector(`[data-virtual-key="${key}"]`);
  if (!(row instanceof HTMLElement)) {
    throw new Error(`Expected the loaded row ${key} to be mounted.`);
  }
  return row;
}

function distanceFromEnd(viewport: HTMLElement) {
  return viewport.scrollHeight - viewport.clientHeight - viewport.scrollTop;
}

async function afterAnimationFrames(count: number): Promise<void> {
  // Adapt the browser frame callback so cancellation checks span measurement reconciliation.
  // oxlint-disable-next-line promise/avoid-new
  await new Promise<void>((resolve) => {
    let remaining = count;
    const nextFrame = () => {
      remaining -= 1;
      if (remaining <= 0) {
        resolve();
        return;
      }
      requestAnimationFrame(nextFrame);
    };
    requestAnimationFrame(nextFrame);
  });
}

const meta = {
  title: "Molecules/VirtualList",
  component: VirtualList<Entry>,
  args: {
    items: createEntries(100, 1000),
    getItemKey: getEntryKey,
    renderItem: renderEntry,
    label: "Example history",
  },
  parameters: {
    docs: {
      description: {
        component:
          "A reusable list with measured row heights, bounded mounted content, stable-key jumps, prepend anchoring, and optional end following. The consumer owns loading and saved reading positions. Browser find and assistive technology can inspect only mounted rows; provide a separate history index or search when every loaded entry must be discoverable.",
      },
    },
  },
  render: () => <HistoryExample />,
} satisfies Meta<typeof VirtualList<Entry>>;

export default meta;
type Story = StoryObj<typeof meta>;

export const ThousandRowsAndJump: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const viewport = canvas.getByRole("region", { name: "Example history" });
    await expect(viewport).toHaveAttribute("tabindex", "0");
    await waitFor(async () => {
      await expect(canvas.getAllByRole("listitem").length).toBeGreaterThan(0);
      await expect(canvas.getAllByRole("listitem").length).toBeLessThan(60);
    });
    await userEvent.click(canvas.getByRole("button", { name: "Jump to entry 500" }));
    await waitFor(async () => {
      await expect(getRow(canvasElement, "row-500")).toHaveFocus();
    });
    await expect(canvas.getAllByRole("listitem").length).toBeLessThan(60);
    await expect(canvas.queryByText("Entry 100", { exact: true })).not.toBeInTheDocument();
    await expect(getRow(canvasElement, "row-500")).toHaveAttribute("aria-setsize", "1000");
  },
};

export const PrependKeepsReadingAnchor: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Jump to entry 500" }));
    await waitFor(async () => {
      await expect(getRow(canvasElement, "row-500")).toHaveFocus();
    });
    const previousTop = getRow(canvasElement, "row-500").getBoundingClientRect().top;
    await userEvent.click(canvas.getByRole("button", { name: "Prepend 100 entries" }));
    await waitFor(async () => {
      const row = getRow(canvasElement, "row-500");
      await expect(Math.abs(row.getBoundingClientRect().top - previousTop)).toBeLessThan(2);
      await expect(row).toHaveAttribute("aria-setsize", "1100");
    });
    await expect(canvas.getAllByRole("listitem").length).toBeLessThan(60);
  },
};

export const PrependRemovesLoadingHeader: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await waitFor(async () => {
      await expect(
        canvas.getByText("Earlier entries appear above the current reading position."),
      ).toBeVisible();
      await expect(getRow(canvasElement, "row-100")).toBeVisible();
    });
    const previousTop = getRow(canvasElement, "row-100").getBoundingClientRect().top;
    await userEvent.click(canvas.getByRole("button", { name: "Prepend 100 entries" }));
    await waitFor(async () => {
      await expect(
        canvas.queryByText("Earlier entries appear above the current reading position."),
      ).not.toBeInTheDocument();
      await expect(
        Math.abs(getRow(canvasElement, "row-100").getBoundingClientRect().top - previousTop),
      ).toBeLessThan(2);
    });
  },
};

export const PrependKeepsLoadingHeader: Story = {
  render: () => <HistoryExample keepHeader />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await waitFor(async () => {
      await expect(getRow(canvasElement, "row-100")).toBeVisible();
    });
    const previousTop = getRow(canvasElement, "row-100").getBoundingClientRect().top;
    await userEvent.click(canvas.getByRole("button", { name: "Prepend 100 entries" }));
    await waitFor(async () => {
      await expect(
        Math.abs(getRow(canvasElement, "row-100").getBoundingClientRect().top - previousTop),
      ).toBeLessThan(2);
    });
  },
};

export const AppendingFollowsOnlyAtEnd: Story = {
  render: () => <HistoryExample initialPosition={{ kind: "end" }} />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const viewport = canvas.getByRole("region", { name: "Example history" });
    await waitFor(async () => {
      await expect(distanceFromEnd(viewport)).toBeLessThan(2);
    });
    await userEvent.click(canvas.getByRole("button", { name: "Append new entry" }));
    await waitFor(async () => {
      await expect(getRow(canvasElement, "row-1100")).toBeVisible();
      await expect(distanceFromEnd(viewport)).toBeLessThan(2);
    });
    await userEvent.click(canvas.getByRole("button", { name: "Jump to entry 500" }));
    await waitFor(async () => {
      await expect(getRow(canvasElement, "row-500")).toHaveFocus();
    });
    const previousTop = getRow(canvasElement, "row-500").getBoundingClientRect().top;
    await userEvent.click(canvas.getByRole("button", { name: "Append new entry" }));
    await afterAnimationFrames(5);
    await expect(
      Math.abs(getRow(canvasElement, "row-500").getBoundingClientRect().top - previousTop),
    ).toBeLessThan(2);
    await expect(distanceFromEnd(viewport)).toBeGreaterThan(1000);
  },
};

export const FirstEntryInEmptyList: Story = {
  render: () => <HistoryExample emptyInitially initialPosition={{ kind: "end" }} />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const viewport = canvas.getByRole("region", { name: "Example history" });
    await userEvent.click(canvas.getByRole("button", { name: "Append new entry" }));
    await waitFor(async () => {
      await expect(getRow(canvasElement, "row-100")).toBeVisible();
      await expect(distanceFromEnd(viewport)).toBeLessThan(2);
    });
  },
};

export const ResizeKeepsTheReadingMode: Story = {
  render: () => <HistoryExample initialPosition={{ kind: "end" }} />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const viewport = canvas.getByRole("region", { name: "Example history" });
    await waitFor(async () => {
      await expect(distanceFromEnd(viewport)).toBeLessThan(2);
    });
    await afterAnimationFrames(2);
    await userEvent.click(canvas.getByRole("button", { name: "Compact viewport" }));
    await waitFor(async () => {
      await expect(viewport.clientHeight).toBeLessThan(150);
      await expect(distanceFromEnd(viewport)).toBeLessThan(2);
    });
    await userEvent.click(canvas.getByRole("button", { name: "Expand viewport" }));
    await waitFor(async () => {
      await expect(viewport.clientHeight).toBeGreaterThan(350);
      await expect(distanceFromEnd(viewport)).toBeLessThan(2);
    });
    await userEvent.click(canvas.getByRole("button", { name: "Jump to entry 500" }));
    await waitFor(async () => {
      await expect(getRow(canvasElement, "row-500")).toHaveFocus();
    });
    const previousTop = getRow(canvasElement, "row-500").getBoundingClientRect().top;
    await userEvent.click(canvas.getByRole("button", { name: "Compact viewport" }));
    await afterAnimationFrames(5);
    await expect(distanceFromEnd(viewport)).toBeGreaterThan(1000);
    await expect(
      Math.abs(getRow(canvasElement, "row-500").getBoundingClientRect().top - previousTop),
    ).toBeLessThan(2);
  },
};

export const WheelCancelsPendingJump: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const viewport = canvas.getByRole("region", { name: "Example history" });
    await userEvent.click(canvas.getByRole("button", { name: "Jump to entry 500" }));
    // Synthetic wheel does not scroll natively; apply its resulting movement explicitly.
    viewport.dispatchEvent(new WheelEvent("wheel", { bubbles: true, deltaY: -100_000 }));
    viewport.scrollTo({ top: 0, behavior: "instant" });
    await waitFor(async () => {
      await expect(getRow(canvasElement, "row-100")).toBeVisible();
    });
    // Wait through measurement reconciliation before asserting the reader retained control.
    await afterAnimationFrames(5);
    await expect(viewport.scrollTop).toBe(0);
    await expect(getRow(canvasElement, "row-100")).toBeVisible();
  },
};

export const GrowingContentFollowsOnlyAtEnd: Story = {
  render: () => <HistoryExample initialPosition={{ kind: "end" }} />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const viewport = canvas.getByRole("region", { name: "Example history" });
    await waitFor(async () => {
      await expect(distanceFromEnd(viewport)).toBeLessThan(2);
    });
    await userEvent.click(canvas.getByRole("button", { name: "Grow latest entry" }));
    await waitFor(async () => {
      await expect(distanceFromEnd(viewport)).toBeLessThan(2);
    });
    await userEvent.click(canvas.getByRole("button", { name: "Jump to entry 500" }));
    await waitFor(async () => {
      await expect(getRow(canvasElement, "row-500")).toHaveFocus();
    });
    const previousTop = getRow(canvasElement, "row-500").getBoundingClientRect().top;
    await userEvent.click(canvas.getByRole("button", { name: "Grow latest entry" }));
    await waitFor(async () => {
      await expect(
        Math.abs(getRow(canvasElement, "row-500").getBoundingClientRect().top - previousTop),
      ).toBeLessThan(2);
      await expect(distanceFromEnd(viewport)).toBeGreaterThan(1000);
    });
  },
};

export const ReturnRestoresStableAnchor: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Jump to entry 500" }));
    await waitFor(async () => {
      await expect(getRow(canvasElement, "row-500")).toHaveFocus();
    });
    const viewport = canvas.getByRole("region", { name: "Example history" });
    viewport.scrollBy({ top: 37, behavior: "instant" });
    await waitFor(async () => {
      const offset =
        viewport.getBoundingClientRect().top -
        getRow(canvasElement, "row-500").getBoundingClientRect().top;
      await expect(offset).toBeGreaterThan(30);
    });
    const previousTop = getRow(canvasElement, "row-500").getBoundingClientRect().top;
    await userEvent.click(canvas.getByRole("button", { name: "Leave list" }));
    await expect(canvas.queryByRole("region", { name: "Example history" })).not.toBeInTheDocument();
    await userEvent.click(canvas.getByRole("button", { name: "Return to list" }));
    await waitFor(async () => {
      await expect(
        Math.abs(getRow(canvasElement, "row-500").getBoundingClientRect().top - previousTop),
      ).toBeLessThan(2);
    });
  },
};

export const JumpRetainsControlFocus: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const control = canvas.getByRole("button", { name: "Jump and retain focus" });
    await userEvent.click(control);
    await waitFor(async () => {
      await expect(getRow(canvasElement, "row-500")).toBeVisible();
      await expect(control).toHaveFocus();
    });
    await expect(getRow(canvasElement, "row-500")).not.toHaveFocus();
  },
};

export const Empty: Story = {
  args: {
    items: [],
    getItemKey: getEntryKey,
    renderItem: renderEntry,
    label: "Empty history",
    empty: <p className="p-4 text-ui text-text-muted">No entries yet.</p>,
    className: "h-40 w-full",
  },
  render: (args) => <VirtualList {...args} />,
};
