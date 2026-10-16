import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/popover";
import { Button } from "./button";
import { Popover } from "./popover";

const meta = {
  title: "Atoms/Popover",
  component: Popover,
  args: { children: null },
} satisfies Meta<typeof Popover>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  render: () => (
    <Popover>
      <Button variant="secondary">Show details</Button>
      <Popover.Content>
        <Popover.Dialog aria-label="Project details">
          <Popover.Heading>Project details</Popover.Heading>
          <p>A shared project for the team.</p>
        </Popover.Dialog>
      </Popover.Content>
    </Popover>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("popover"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
