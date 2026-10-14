import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { Field } from "../molecules/field";
import { Button } from "./button";
import { NativeSelect } from "./native-select";

const meta = {
  title: "Atoms/NativeSelect",
  component: NativeSelect,
  args: {
    name: "cadence",
    defaultValue: "weekly",
    children: (
      <>
        <option value="daily">Daily</option>
        <option value="weekly">Weekly</option>
        <option value="never">Never</option>
      </>
    ),
  },
  render: (args) => (
    <Field
      id="notification-cadence"
      label="Notification cadence"
      helper="Choose how often to receive updates."
      className="max-w-sm"
    >
      {(control) => <NativeSelect {...args} {...control} />}
    </Field>
  ),
  parameters: {
    docs: {
      description: {
        component:
          "A native select keeps the browser’s keyboard and mobile picker behavior. Supply a label with Field or a native label; the consuming feature owns options, values, and translations.",
      },
    },
  },
} satisfies Meta<typeof NativeSelect>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Compact: Story = { args: { controlSize: "compact" } };

export const QuietRound: Story = {
  args: { tone: "quiet" },
};

export const Disabled: Story = {
  args: { disabled: true },
  play: async ({ canvasElement }) => {
    await expect(
      within(canvasElement).getByRole("combobox", { name: "Notification cadence" }),
    ).toBeDisabled();
  },
};

export const KeyboardAndSelection: Story = {
  render: (args) => (
    <form
      className="flex max-w-sm flex-col items-start gap-5"
      onSubmit={(event) => {
        event.preventDefault();
      }}
    >
      <Field
        id="keyboard-cadence"
        label="Notification cadence"
        helper="Choose how often to receive updates."
        className="w-full"
      >
        {(control) => <NativeSelect {...args} {...control} />}
      </Field>
      <Button type="submit">Save preferences</Button>
    </form>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const select = canvas.getByRole("combobox", { name: "Notification cadence" });
    await userEvent.click(canvas.getByText("Notification cadence"));
    await expect(select).toHaveFocus();
    await expect(select).toHaveAccessibleDescription("Choose how often to receive updates.");
    await userEvent.selectOptions(select, "daily");
    await expect(select).toHaveValue("daily");
    await userEvent.tab();
    await expect(canvas.getByRole("button", { name: "Save preferences" })).toHaveFocus();
  },
};
