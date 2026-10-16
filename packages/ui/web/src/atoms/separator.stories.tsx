import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/separator";
import { Separator } from "./separator";

const meta = {
  title: "Atoms/Separator",
  component: Separator,
} satisfies Meta<typeof Separator>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Horizontal: Story = {
  render: () => (
    <div className="flex max-w-lg flex-col gap-5">
      <p className="font-medium">Shared work</p>
      <Separator />
      <p className="text-ui text-text-muted">A quiet boundary between related sections.</p>
    </div>
  ),
};

export const Vertical: Story = {
  render: () => (
    <div className="flex h-8 items-center gap-5 text-ui">
      <span>Swarmie</span>
      <Separator orientation="vertical" />
      <span className="text-text-muted">by Syntropika</span>
    </div>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("separator"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
