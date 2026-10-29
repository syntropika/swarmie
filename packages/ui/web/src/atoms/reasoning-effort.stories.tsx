import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, userEvent, within } from "storybook/test";

import { ReasoningEffort } from "./reasoning-effort";
import type { ReasoningEffortProps } from "./reasoning-effort";

const levels = [
  { id: "low", label: "Low" },
  { id: "medium", label: "Medium" },
  { id: "high", label: "High" },
  { id: "xhigh", label: "Extra high" },
];

const meta = {
  title: "Atoms/ReasoningEffort",
  component: ReasoningEffort,
  args: { levels, value: "medium", model: "Example model", onChange: () => undefined },
  parameters: {
    docs: {
      description: {
        component:
          "A stock HeroUI slider maps each step to a supported reasoning level. The selected level and model remain visible and available to assistive technology.",
      },
    },
  },
} satisfies Meta<typeof ReasoningEffort>;
export default meta;
type Story = StoryObj<typeof meta>;

function InteractiveReasoning(args: ReasoningEffortProps) {
  const [value, setValue] = useState(args.value);
  return (
    <div className="w-64">
      <ReasoningEffort {...args} value={value} onChange={setValue} />
    </div>
  );
}
export const Default: Story = {
  render: (args) => <InteractiveReasoning {...args} />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const slider = canvas.getByRole("slider", { name: /Thinking\s*:\s*Medium/u });
    slider.focus();
    await userEvent.keyboard("{ArrowRight}");
    await expect(canvas.getByRole("slider", { name: /Thinking\s*:\s*High/u })).toHaveValue("2");
    await userEvent.keyboard("{Home}");
    await expect(canvas.getByRole("slider", { name: /Thinking\s*:\s*Low/u })).toHaveValue("0");
    await userEvent.keyboard("{End}");
    await expect(canvas.getByRole("slider", { name: /Thinking\s*:\s*Extra high/u })).toHaveValue(
      "3",
    );
  },
};
export const Disabled: Story = { args: { isDisabled: true } };
export const Unavailable: Story = { args: { levels: [], value: "" } };
export const SingleLevel: Story = {
  args: { levels: [{ id: "high", label: "High" }], value: "high" },
};

export const UnknownValue: Story = { args: { value: "unsupported" } };
