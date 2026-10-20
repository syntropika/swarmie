import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/spinner";
import { Spinner } from "./spinner";

const meta = {
  title: "Atoms/Spinner",
  component: Spinner,
  args: { children: null },
} satisfies Meta<typeof Spinner>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  render: () => (
    <div className="flex gap-4">
      <Spinner size="sm" aria-label="Loading small preview" />
      <Spinner aria-label="Loading preview" />
      <Spinner size="lg" aria-label="Loading large preview" />
    </div>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("spinner"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
