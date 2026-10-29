import type { Meta, StoryObj } from "@storybook/react-vite";

import { ReasoningIndicator } from "./reasoning-indicator";

const levels = ["quick", "balanced", "thorough"];
const meta = {
  title: "Atoms/ReasoningIndicator",
  component: ReasoningIndicator,
  args: { levels, value: "balanced" },
  parameters: {
    docs: {
      description: {
        component:
          "The Composer and provider settings share this decorative ring. Its arc shows relative position in a model's own ordered reasoning levels. Always pair it with a visible label.",
      },
    },
  },
} satisfies Meta<typeof ReasoningIndicator>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  render: (args) => (
    <span className="flex items-center gap-2">
      <ReasoningIndicator {...args} />
      {args.value}
    </span>
  ),
};
export const ModelScale: Story = {
  render: () => (
    <ol className="flex gap-4">
      {levels.map((value) => (
        <li key={value} className="grid justify-items-center gap-2">
          <ReasoningIndicator levels={levels} value={value} />
          <span>{value}</span>
        </li>
      ))}
    </ol>
  ),
};
export const NumericBudgets: Story = {
  args: { levels: ["1024 tokens", "4096 tokens", "16384 tokens"], value: "4096 tokens" },
  render: (args) => (
    <span className="flex items-center gap-2">
      <ReasoningIndicator {...args} />
      {args.value}
    </span>
  ),
};
