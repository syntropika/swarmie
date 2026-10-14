import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ThemePreferenceControl } from "./theme-preference-control";
import type { ThemePreferenceControlProps } from "./theme-preference-control";

function ControlledPreference(args: ThemePreferenceControlProps) {
  const [preference, setPreference] = useState(args.value);
  return <ThemePreferenceControl {...args} value={preference} onValueChange={setPreference} />;
}

const meta = {
  title: "Molecules/ThemePreferenceControl",
  component: ThemePreferenceControl,
  args: {
    value: "system",
    label: "Appearance",
    lightLabel: "Light",
    darkLabel: "Dark",
    systemLabel: "System",
    onValueChange: () => undefined,
  },
  render: (args) => <ControlledPreference {...args} />,
  decorators: [
    (Story) => (
      <div className="w-full max-w-xs">
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof ThemePreferenceControl>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const select = canvas.getByRole("combobox", { name: "Appearance" });
    await expect(select).toHaveAccessibleName("Appearance");
    const choose = async (name: string) => {
      await userEvent.click(select);
      const option = within(canvasElement.ownerDocument.body).getByRole("option", {
        name,
      });
      await waitFor(async () => {
        await expect(option).toBeVisible();
      });
      await userEvent.click(option);
      await expect(select).toHaveTextContent(name);
      await waitFor(async () => {
        await expect(select).toHaveFocus();
      });
    };
    await choose("Dark");
    await choose("Light");
    await choose("System");
  },
};

export const Footer: Story = {
  args: { tone: "footer" },
  decorators: [
    (Story) => (
      <div className="rounded-control bg-footer p-6 text-footer-text">
        <Story />
      </div>
    ),
  ],
};

export const LongLabels: Story = {
  args: {
    label: "Choose your preferred appearance",
    systemLabel: "Use the system appearance",
  },
};

export const SettingsRow: Story = {
  args: { layout: "row" },
};
