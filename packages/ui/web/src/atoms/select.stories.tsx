import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { Field } from "../molecules/field";
import { Button } from "./button";
import { GlobeIcon } from "./globe-icon";
import { Icon } from "./icon";
import {
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectRoot,
  SelectTrigger,
  SelectValue,
} from "./select";

const meta = {
  title: "Atoms/Select",
  component: SelectRoot,
  args: { defaultValue: "weekly" },
  render: (args) => (
    <div className="flex max-w-sm flex-col items-start gap-5">
      <Field
        id="notification-cadence"
        label="Notification cadence"
        helper="Choose how often to receive updates."
        className="w-full"
      >
        {(control) => (
          <SelectRoot {...args}>
            <SelectTrigger {...control} className="w-full">
              <SelectValue placeholder="Choose a cadence" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="daily">Daily</SelectItem>
              <SelectItem value="weekly">Weekly</SelectItem>
              <SelectItem value="yearly" disabled>
                Yearly (unavailable)
              </SelectItem>
              <SelectItem value="monthly">Monthly</SelectItem>
            </SelectContent>
          </SelectRoot>
        )}
      </Field>
      <Button variant="secondary">Save preferences</Button>
    </div>
  ),
  parameters: {
    docs: {
      description: {
        component:
          "A composed select with a styled popup, managed focus, keyboard navigation, typeahead, and disabled options. Use NativeSelect when the platform picker is preferred. The consumer owns values, labels, and application behavior.",
      },
    },
  },
} satisfies Meta<typeof SelectRoot>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const DetailedOptions: Story = {
  render: () => (
    <SelectRoot defaultValue="balanced">
      <SelectTrigger aria-label="Model profile">
        <Icon name="agent" />
        <SelectValue />
      </SelectTrigger>
      <SelectContent className="w-80">
        <SelectGroup>
          <SelectLabel>Workspace models</SelectLabel>
          <SelectItem
            value="balanced"
            description="Everyday work with a quick response"
            leading={<Icon name="chat" />}
          >
            Balanced
          </SelectItem>
          <SelectItem
            value="deep"
            description="More time for complex decisions"
            leading={<Icon name="memory" />}
          >
            Deep reasoning
          </SelectItem>
          <SelectItem value="restricted" disabled description="Requires workspace access">
            Restricted
          </SelectItem>
        </SelectGroup>
      </SelectContent>
    </SelectRoot>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const trigger = canvas.getByRole("combobox", { name: "Model profile" });
    await userEvent.click(trigger);
    await expect(await page.findByRole("option", { name: "Restricted" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await expect(page.getByRole("option", { name: "Deep reasoning" })).toHaveAccessibleDescription(
      "More time for complex decisions",
    );
    await waitFor(async () => {
      await expect(page.getByRole("option", { name: "Balanced" })).toHaveFocus();
    });
    await userEvent.keyboard("{End}");
    await waitFor(async () => {
      await expect(page.getByRole("option", { name: "Deep reasoning" })).toHaveFocus();
    });
    await userEvent.keyboard("{Enter}");
    await waitFor(async () => {
      await expect(trigger).toHaveFocus();
      await expect(trigger).toHaveTextContent("Deep reasoning");
    });
    await expect(trigger).not.toHaveTextContent("More time");
  },
};

export const Ghost: Story = {
  args: { defaultValue: "en" },
  render: (args) => (
    <SelectRoot {...args}>
      <SelectTrigger aria-label="Language" variant="ghost">
        <GlobeIcon />
        <SelectValue />
      </SelectTrigger>
      <SelectContent align="end">
        <SelectItem value="en">English</SelectItem>
        <SelectItem value="es">Spanish</SelectItem>
        <SelectItem value="fr">French</SelectItem>
      </SelectContent>
    </SelectRoot>
  ),
};

export const Inverse: Story = {
  args: { defaultValue: "en" },
  render: (args) => (
    <div className="flex min-h-80 items-end justify-end rounded-panel bg-primary p-6">
      <SelectRoot {...args}>
        <SelectTrigger aria-label="Language" variant="ghost" tone="inverse">
          <GlobeIcon />
          <SelectValue />
        </SelectTrigger>
        <SelectContent align="end" side="top">
          <SelectItem value="en">English</SelectItem>
          <SelectItem value="es">Spanish</SelectItem>
          <SelectItem value="fr">French</SelectItem>
        </SelectContent>
      </SelectRoot>
    </div>
  ),
};

export const Disabled: Story = {
  args: { disabled: true },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const trigger = canvas.getByRole("combobox", { name: "Notification cadence" });
    await expect(trigger).toBeDisabled();
    await userEvent.tab();
    await expect(canvas.getByRole("button", { name: "Save preferences" })).toHaveFocus();
  },
};

export const KeyboardAndSelection: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const trigger = canvas.getByRole("combobox", { name: "Notification cadence" });
    await userEvent.tab();
    await expect(trigger).toHaveFocus();
    await expect(trigger).toHaveAccessibleDescription("Choose how often to receive updates.");
    await userEvent.keyboard("{ArrowDown}");
    await waitFor(async () => {
      await expect(page.getByRole("option", { name: "Weekly" })).toHaveFocus();
    });
    await userEvent.keyboard("{ArrowDown}");
    await waitFor(async () => {
      await expect(page.getByRole("option", { name: "Monthly" })).toHaveFocus();
    });
    await expect(page.getByRole("option", { name: "Yearly (unavailable)" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await userEvent.keyboard("{Home}");
    await waitFor(async () => {
      await expect(page.getByRole("option", { name: "Daily" })).toHaveFocus();
    });
    await userEvent.keyboard("{End}");
    await waitFor(async () => {
      await expect(page.getByRole("option", { name: "Monthly" })).toHaveFocus();
    });
    await userEvent.keyboard("{Enter}");
    await waitFor(async () => {
      await expect(trigger).toHaveFocus();
    });
    await expect(trigger).toHaveTextContent("Monthly");
    await expect(trigger).toHaveAttribute("aria-expanded", "false");
    await expect(page.queryByRole("listbox")).not.toBeInTheDocument();
    await userEvent.tab();
    await expect(canvas.getByRole("button", { name: "Save preferences" })).toHaveFocus();
  },
};

export const TypeaheadAndEscape: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const trigger = canvas.getByRole("combobox", { name: "Notification cadence" });
    await userEvent.tab();
    await expect(trigger).toHaveFocus();
    await userEvent.keyboard("{Enter}");
    await waitFor(async () => {
      await expect(page.getByRole("option", { name: "Weekly" })).toHaveFocus();
    });
    await userEvent.keyboard("m");
    await waitFor(async () => {
      await expect(page.getByRole("option", { name: "Monthly" })).toHaveFocus();
    });
    await userEvent.keyboard("{Escape}");
    await waitFor(async () => {
      await expect(trigger).toHaveFocus();
    });
    await expect(trigger).toHaveTextContent("Weekly");
    await expect(trigger).toHaveAttribute("aria-expanded", "false");
    await expect(page.queryByRole("listbox")).not.toBeInTheDocument();
  },
};

export const PointerSelection: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const trigger = canvas.getByRole("combobox", { name: "Notification cadence" });
    await userEvent.click(trigger);
    await userEvent.click(await page.findByRole("option", { name: "Daily" }));
    await waitFor(async () => {
      await expect(trigger).toHaveTextContent("Daily");
    });
    await expect(trigger).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(trigger);
    await expect(page.getByRole("option", { name: "Daily" })).toHaveAttribute(
      "data-state",
      "checked",
    );
    await userEvent.keyboard("{Escape}");
    await waitFor(async () => {
      await expect(trigger).toHaveFocus();
    });
  },
};
