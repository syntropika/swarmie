import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, fn, userEvent, within } from "storybook/test";

import { ReasoningEffortSlider } from "./reasoning-effort-slider";

const options = [
  { value: "low", label: "Low" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
  { value: "extra-high", label: "Extra high" },
];

function Example({ disabled = false }: { disabled?: boolean }) {
  const [value, setValue] = useState("medium");
  return (
    <div className="max-w-96 space-y-4 p-6">
      <ReasoningEffortSlider
        label="Reasoning effort"
        model="Swarmie"
        value={value}
        onChange={setValue}
        options={options}
        disabled={disabled}
      />
      <output className="text-ui text-text-muted">Selected effort: {value}</output>
    </div>
  );
}

const meta = {
  title: "Assistant/Reasoning/ReasoningEffortSlider",
  component: ReasoningEffortSlider,
  args: { label: "Reasoning effort", value: "medium", options, onChange: fn<() => void>() },
  parameters: {
    docs: {
      description: {
        component:
          "The AICSS continuously draggable slider, morphing shoulder, widening thumb, label reveal, and discrete snapping adapted to Swarmie tokens. Selection is controlled; keyboard arrows, Home, and End use native range semantics. Reduced motion resolves geometry immediately.",
      },
    },
  },
  render: () => <Example />,
} satisfies Meta<typeof ReasoningEffortSlider>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Slider: Story = {};

export const KeyboardSelection: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const slider = canvas.getByRole("slider", { name: "Reasoning effort" });
    slider.focus();
    await userEvent.keyboard("{ArrowRight}");
    await expect(slider).toHaveAttribute("aria-valuetext", "Swarmie High");
    await userEvent.keyboard("{End}");
    await expect(canvas.getByText("Selected effort: extra-high")).toBeVisible();
    await userEvent.keyboard("{Home}");
    await expect(canvas.getByText("Selected effort: low")).toBeVisible();
  },
};

export const Disabled: Story = { render: () => <Example disabled /> };
