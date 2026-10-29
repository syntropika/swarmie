import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/switch";
import { Switch } from "./switch";

const meta = {
  title: "Atoms/Switch",
  component: Switch,
  args: { isDisabled: false, defaultSelected: false },
  render: (args) => (
    <Switch {...args}>
      <Switch.Content>
        <Switch.Control>
          <Switch.Thumb />
        </Switch.Control>
        Notify me about updates
      </Switch.Content>
    </Switch>
  ),
} satisfies Meta<typeof Switch>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {};
export const Selected: Story = { args: { defaultSelected: true } };
export const Disabled: Story = { args: { isDisabled: true } };
export const DisabledSelected: Story = { args: { isDisabled: true, defaultSelected: true } };

export const IconOnlyTouchTarget: Story = {
  render: (args) => (
    <Switch {...args} aria-label="Enable notifications">
      <Switch.Content>
        <Switch.Control>
          <Switch.Thumb />
        </Switch.Control>
      </Switch.Content>
    </Switch>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("switch"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
