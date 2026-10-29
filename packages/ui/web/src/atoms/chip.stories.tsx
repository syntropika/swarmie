import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/chip";
import { Chip } from "./chip";

const meta = {
  title: "Atoms/Chip",
  component: Chip,
  args: { children: "Agent", color: "default" },
} satisfies Meta<typeof Chip>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Tones: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-3">
      <Chip>Member</Chip>
      <Chip color="accent">Agent</Chip>
      <Chip color="success">Saved</Chip>
      <Chip color="warning">Review needed</Chip>
      <Chip color="danger">Could not save</Chip>
    </div>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("chip"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
