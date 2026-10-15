import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, fn, userEvent, waitFor, within } from "storybook/test";

import { PositionNavigator } from "./position-navigator";
import type { PositionNavigatorProps } from "./position-navigator";

const positions = Array.from({ length: 40 }, (_, index) => ({
  id: `position-${index + 1}`,
  label: `Position ${index + 1}`,
  preview: `A plain-text preview of the content at position ${index + 1}. The consumer supplies this text and owns the destination.`,
}));

function ControlledNavigator(args: PositionNavigatorProps) {
  const [activeId, setActiveId] = useState(args.activeId);
  return (
    <div className="flex min-h-96 max-w-2xl flex-col gap-4 md:flex-row">
      <PositionNavigator
        {...args}
        activeId={activeId}
        onSelect={(id) => {
          setActiveId(id);
          args.onSelect(id);
        }}
      />
      <div className="min-w-0 flex-1 rounded-panel bg-surface-muted p-6">
        <p className="font-medium">Document content</p>
        <p className="mt-2 text-ui text-text-muted">
          Select a mark to navigate. Arrow keys move between marks; Enter or Space selects one. Home
          and End reach the first and last positions. Escape dismisses the preview.
        </p>
        <output className="mt-4 block text-ui">Selected: {activeId ?? "none"}</output>
      </div>
    </div>
  );
}

const meta = {
  title: "Molecules/PositionNavigator",
  component: PositionNavigator,
  args: {
    items: positions,
    activeId: "position-1",
    label: "Document positions",
    onSelect: fn<(id: string) => void>(),
  },
  render: (args) => <ControlledNavigator {...args} />,
  parameters: {
    docs: {
      description: {
        component:
          "A compact virtualized desktop rail with 12px marker spacing and plain-text previews. Positions have stable consumer-owned IDs. Visible positions show the document's viewport coverage; activeId identifies its current position. The active, focused, and hovered marks remain mounted. The consumer owns grouping, labels, available space, and navigation. Below 768px the navigator is not mounted.",
      },
    },
  },
} satisfies Meta<typeof PositionNavigator>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const VisibleRange: Story = {
  args: {
    activeId: "position-12",
    visibleIds: ["position-12", "position-13", "position-14"],
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    if (await verifyMobileAbsence(canvasElement)) {
      return;
    }
    const first = canvas.getByRole("button", { name: "Position 12" });
    const second = canvas.getByRole("button", { name: "Position 13" });
    await expect(first).toHaveAttribute("aria-current", "location");
    await expect(second).not.toHaveAttribute("aria-current");
    await expect(second).toHaveAttribute("data-visible", "true");
    await expect(canvas.getByRole("button", { name: "Position 14" })).toHaveAttribute(
      "data-visible",
      "true",
    );
    await expect(second.getBoundingClientRect().top - first.getBoundingClientRect().top).toBe(12);
    await expect(first.getBoundingClientRect().height).toBe(12);
    const bounds = first.getBoundingClientRect();
    await expect(first.contains(document.elementFromPoint(bounds.left + 22, bounds.top + 6))).toBe(
      true,
    );
  },
};

export const ConstrainedHeight: Story = {
  render: (args) => (
    <div className="flex h-24 flex-col">
      <PositionNavigator {...args} className="flex max-h-full min-h-0 flex-col" />
    </div>
  ),
  play: async ({ canvasElement, args }) => {
    const canvas = within(canvasElement);
    if (await verifyMobileAbsence(canvasElement)) {
      return;
    }
    const rail = canvas.getByRole("navigation", { name: "Document positions" });
    await expect(rail.getBoundingClientRect().height).toBeLessThanOrEqual(96);
    await userEvent.tab();
    await userEvent.keyboard("{End}");
    await waitFor(async () => {
      await expect(canvas.getByRole("button", { name: "Position 40" })).toHaveFocus();
    });
    await expect(rail.getBoundingClientRect().height).toBeLessThanOrEqual(96);
    const track = rail.querySelector("div");
    if (track === null) {
      throw new Error("The navigator must expose its scroll track.");
    }
    track.scrollTo({ top: 0, behavior: "instant" });
    await waitFor(async () => {
      await expect(canvas.getByRole("tooltip", { hidden: true })).not.toBeVisible();
    });
    await expect(args.onSelect).not.toHaveBeenCalled();
  },
};

async function verifyMobileAbsence(canvasElement: HTMLElement) {
  if (window.matchMedia("(min-width: 768px)").matches) {
    return false;
  }
  const canvas = within(canvasElement);
  await expect(
    canvas.queryByRole("navigation", { name: "Document positions" }),
  ).not.toBeInTheDocument();
  await expect(canvas.queryByRole("combobox")).not.toBeInTheDocument();
  await expect(canvas.queryAllByRole("option")).toHaveLength(0);
  await expect(canvas.queryAllByRole("button")).toHaveLength(0);
  return true;
}

export const KeyboardAndPreview: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    if (await verifyMobileAbsence(canvasElement)) {
      return;
    }

    await userEvent.tab();
    await expect(canvas.getByRole("button", { name: "Position 1" })).toHaveFocus();
    await expect(canvas.getByRole("tooltip")).toHaveTextContent("content at position 1");
    await userEvent.keyboard("{ArrowDown}");
    await expect(canvas.getByRole("button", { name: "Position 2" })).toHaveFocus();
    await userEvent.keyboard("{Enter}");
    await expect(canvas.getByRole("button", { name: "Position 2" })).toHaveAttribute(
      "aria-current",
      "location",
    );
    await userEvent.keyboard("{Escape}");
    await expect(canvas.queryByRole("tooltip")).not.toBeInTheDocument();
    await userEvent.keyboard("{End}");
    await waitFor(async () => {
      await expect(canvas.getByRole("button", { name: "Position 40" })).toHaveFocus();
    });
    await userEvent.keyboard(" ");
    await expect(canvas.getByRole("status")).toHaveTextContent("Selected: position-40");
    await userEvent.keyboard("{Home}");
    await waitFor(async () => {
      await expect(canvas.getByRole("button", { name: "Position 1" })).toHaveFocus();
    });
    await userEvent.keyboard("{Escape}");
  },
};

export const HoverPreview: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    if (await verifyMobileAbsence(canvasElement)) {
      return;
    }
    const mark = canvas.getByRole("button", { name: "Position 1" });
    await userEvent.hover(mark);
    await expect(canvas.getByRole("tooltip")).toHaveTextContent("content at position 1");
    await userEvent.hover(canvas.getByRole("tooltip"));
    await expect(canvas.getByRole("tooltip")).toBeVisible();
    await userEvent.keyboard("{Escape}");
    await expect(canvas.queryByRole("tooltip")).not.toBeInTheDocument();
  },
};

export const LongHistory: Story = {
  args: {
    items: Array.from({ length: 1000 }, (_, index) => ({
      id: `position-${index + 1}`,
      label: `Position ${index + 1}`,
      preview: `Content preview for position ${index + 1}.`,
    })),
    activeId: "position-500",
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    if (await verifyMobileAbsence(canvasElement)) {
      return;
    }

    await userEvent.tab();
    await expect(canvas.getByRole("button", { name: "Position 500" })).toHaveFocus();
    const mountedLimit = Math.ceil((window.innerHeight * 0.45) / 12) + 16;
    await expect(canvas.getAllByRole("button").length).toBeLessThanOrEqual(mountedLimit);
    await userEvent.keyboard("{End}");
    await waitFor(async () => {
      await expect(canvas.getByRole("button", { name: "Position 1000" })).toHaveFocus();
    });
    await expect(canvas.getAllByRole("button").length).toBeLessThanOrEqual(mountedLimit);
    await userEvent.keyboard("{Enter}");
    await expect(canvas.getByRole("status")).toHaveTextContent("Selected: position-1000");
    await userEvent.keyboard("{Home}");
    await waitFor(async () => {
      await expect(canvas.getByRole("button", { name: "Position 1" })).toHaveFocus();
    });
    await expect(canvas.getAllByRole("button").length).toBeLessThanOrEqual(mountedLimit);
  },
};

export const Empty: Story = { args: { items: [] } };
