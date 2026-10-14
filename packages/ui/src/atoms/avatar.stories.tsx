import type { Meta, StoryObj } from "@storybook/react-vite";

import { Avatar } from "./avatar";
import { Badge } from "./badge";

const meta = {
  title: "Atoms/Avatar",
  component: Avatar,
  args: { kind: "person", initials: "JL", label: "Jamie Lee" },
  parameters: {
    docs: {
      description: {
        component:
          "People use circular avatars; Agents use softly squared identity tiles and a visible Agent label beside their name. Workspaces use softly squared mint tiles. Identity colors do not describe availability or execution state.",
      },
    },
  },
} satisfies Meta<typeof Avatar>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Person: Story = {};

export const Small: Story = { args: { size: "sm" } };

export const Workspace: Story = {
  args: { kind: "workspace", initials: "S", label: "Studio" },
  render: (args) => (
    <div className="flex items-center gap-3">
      <Avatar {...args} />
      <span className="text-ui font-medium">Studio</span>
    </div>
  ),
};

export const Agent: Story = {
  args: { kind: "agent", initials: "SY", label: "Sydney" },
  render: (args) => (
    <div className="flex items-center gap-3">
      <Avatar {...args} />
      <span className="text-ui font-medium">Sydney</span>
      <Badge tone="agent">Agent</Badge>
    </div>
  ),
};

export const Participants: Story = {
  render: () => (
    <div className="flex flex-wrap gap-8">
      <div className="flex items-center gap-3">
        <Avatar kind="person" initials="JL" label="Jamie Lee" />
        <span className="text-ui font-medium">Jamie Lee</span>
      </div>
      <div className="flex items-center gap-3">
        <Avatar kind="person" initials="AK" label="Alex Kim" />
        <span className="text-ui font-medium">Alex Kim</span>
      </div>
      <div className="flex items-center gap-3">
        <Avatar kind="agent" initials="SY" label="Sydney" />
        <span className="text-ui font-medium">Sydney</span>
        <Badge tone="agent">Agent</Badge>
      </div>
    </div>
  ),
};
