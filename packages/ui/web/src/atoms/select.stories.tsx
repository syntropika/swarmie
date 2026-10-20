import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/select";
import { Select, ListBox, Label, Description } from "./select";

const meta = {
  title: "Atoms/Select",
  component: Select,
  args: { defaultValue: "weekly", isDisabled: false },
  render: (args) => (
    <Select {...args} className="max-w-sm" placeholder="Choose a cadence">
      <Label>Notification cadence</Label>
      <Select.Trigger>
        <Select.Value />
        <Select.Indicator />
      </Select.Trigger>
      <Description>Choose how often to receive updates.</Description>
      <Select.Popover>
        <ListBox>
          <ListBox.Item id="daily" textValue="Daily">
            Daily
            <ListBox.ItemIndicator />
          </ListBox.Item>
          <ListBox.Item id="weekly" textValue="Weekly">
            Weekly
            <ListBox.ItemIndicator />
          </ListBox.Item>
          <ListBox.Item id="yearly" textValue="Yearly" isDisabled>
            Yearly (unavailable)
            <ListBox.ItemIndicator />
          </ListBox.Item>
          <ListBox.Item id="monthly" textValue="Monthly">
            Monthly
            <ListBox.ItemIndicator />
          </ListBox.Item>
        </ListBox>
      </Select.Popover>
    </Select>
  ),
} satisfies Meta<typeof Select>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {};
export const Secondary: Story = { args: { variant: "secondary" } };
export const Disabled: Story = { args: { isDisabled: true } };
export const KeyboardAndSelection: Story = {};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("select"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
