import type { Meta, StoryObj } from "@storybook/react-vite";

import { Badge } from "./badge";

const meta = {
  title: "Atoms/Badge",
  component: Badge,
  args: { children: "Agent", tone: "agent" },
} satisfies Meta<typeof Badge>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Tones: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-3">
      <Badge>Member</Badge>
      <Badge tone="agent">Agent</Badge>
      <Badge tone="success">Saved</Badge>
      <Badge tone="attention">Review needed</Badge>
      <Badge tone="danger">Could not save</Badge>
    </div>
  ),
};
