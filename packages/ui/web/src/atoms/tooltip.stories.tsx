import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/tooltip";
import { Button } from "./button";
import { Icon } from "./icon";
import { Tooltip } from "./tooltip";

const meta = {
  title: "Atoms/Tooltip",
  component: Tooltip,
  args: { children: null },
  parameters: { layout: "centered" },
} satisfies Meta<typeof Tooltip>;
export default meta;
type Story = StoryObj<typeof meta>;
export const KeyboardDisclosure: Story = {
  render: () => (
    <Tooltip delay={0}>
      <Button variant="tertiary" size="sm" isIconOnly aria-label="Explain this control">
        <Icon name="circle-attention" />
      </Button>
      <Tooltip.Content placement="bottom">A short explanation of the control.</Tooltip.Content>
    </Tooltip>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("tooltip"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
