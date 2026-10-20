import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/avatar";
import { Avatar } from "./avatar";

const meta = {
  title: "Atoms/Avatar",
  component: Avatar,
  args: { "aria-label": "Jamie Lee", children: <Avatar.Fallback>JL</Avatar.Fallback> },
} satisfies Meta<typeof Avatar>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {};
export const Sizes: Story = {
  render: () => (
    <div className="flex items-center gap-3">
      <Avatar size="sm" aria-label="Small avatar">
        <Avatar.Fallback>SM</Avatar.Fallback>
      </Avatar>
      <Avatar aria-label="Medium avatar">
        <Avatar.Fallback>MD</Avatar.Fallback>
      </Avatar>
      <Avatar size="lg" aria-label="Large avatar">
        <Avatar.Fallback>LG</Avatar.Fallback>
      </Avatar>
    </div>
  ),
};
export const Colors: Story = {
  render: () => (
    <div className="flex flex-wrap gap-3">
      <Avatar aria-label="Jamie Lee">
        <Avatar.Fallback>JL</Avatar.Fallback>
      </Avatar>
      <Avatar color="accent" aria-label="Swarmie agent">
        <Avatar.Fallback>SW</Avatar.Fallback>
      </Avatar>
      <Avatar color="success" aria-label="Studio workspace">
        <Avatar.Fallback>S</Avatar.Fallback>
      </Avatar>
    </div>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("avatar"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
