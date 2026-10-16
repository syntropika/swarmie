import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/disclosure";
import { Button } from "./button";
import { Disclosure } from "./disclosure";

const meta = {
  title: "Atoms/Disclosure",
  component: Disclosure,
  args: { children: null },
  render: (args) => (
    <Disclosure {...args}>
      <Disclosure.Heading>
        <Button slot="trigger" variant="secondary">
          Project activity
          <Disclosure.Indicator />
        </Button>
      </Disclosure.Heading>
      <Disclosure.Content>
        <Disclosure.Body>Three changes are ready for review.</Disclosure.Body>
      </Disclosure.Content>
    </Disclosure>
  ),
} satisfies Meta<typeof Disclosure>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {};
export const Expanded: Story = { args: { defaultExpanded: true } };
export const Disabled: Story = { args: { isDisabled: true } };

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("disclosure"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
