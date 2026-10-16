import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/radio-group";
import { RadioGroup, Radio } from "./radio-group";
import { Label } from "./select";

const meta = {
  title: "Atoms/RadioGroup",
  component: RadioGroup,
  args: { name: "length", defaultValue: "brief" },
  render: (args) => (
    <RadioGroup {...args}>
      <Label>Response length</Label>
      <Radio value="brief">
        <Radio.Content>
          <Radio.Control>
            <Radio.Indicator />
          </Radio.Control>
          Brief
        </Radio.Content>
      </Radio>
      <Radio value="detailed">
        <Radio.Content>
          <Radio.Control>
            <Radio.Indicator />
          </Radio.Control>
          Detailed
        </Radio.Content>
      </Radio>
      <Radio value="unavailable" isDisabled>
        <Radio.Content>
          <Radio.Control>
            <Radio.Indicator />
          </Radio.Control>
          Unavailable
        </Radio.Content>
      </Radio>
    </RadioGroup>
  ),
} satisfies Meta<typeof RadioGroup>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {};
export const Secondary: Story = { args: { variant: "secondary" } };
export const Keyboard: Story = {};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("radio-group"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
