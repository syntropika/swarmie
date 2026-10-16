import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/input-group";
import { InputGroup } from "./input-group";

const meta = {
  title: "Atoms/InputGroup",
  component: InputGroup,
  args: { children: null },
} satisfies Meta<typeof InputGroup>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  render: () => (
    <InputGroup className="max-w-sm">
      <InputGroup.Prefix aria-hidden="true">@</InputGroup.Prefix>
      <InputGroup.Input aria-label="Username" placeholder="username" />
    </InputGroup>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("input-group"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
