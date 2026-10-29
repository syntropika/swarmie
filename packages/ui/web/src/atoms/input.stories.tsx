import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/input";
import { Button } from "./button";
import { Icon } from "./icon";
import { Input } from "./input";
import { InputGroup } from "./input-group";
import { TextField, Label } from "./text-field";

const meta = {
  title: "Atoms/Input",
  component: Input,
  args: { placeholder: "name@example.com", type: "email", autoComplete: "email" },
  render: (args) => (
    <TextField>
      <Label>Email address</Label>
      <Input {...args} />
    </TextField>
  ),
} satisfies Meta<typeof Input>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const ReadOnly: Story = { args: { defaultValue: "alex@example.com", readOnly: true } };

export const Disabled: Story = { args: { defaultValue: "alex@example.com", disabled: true } };

export const TrailingAction: Story = {
  render: () => (
    <TextField>
      <Label>Search</Label>
      <InputGroup>
        <InputGroup.Input type="search" />
        <InputGroup.Suffix>
          <Button variant="tertiary" isIconOnly aria-label="Submit search">
            <Icon name="arrow" />
          </Button>
        </InputGroup.Suffix>
      </InputGroup>
    </TextField>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("input"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
