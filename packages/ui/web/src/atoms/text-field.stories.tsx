import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/text-field";
import { Input } from "./input";
import { TextField, Label, Description, FieldError } from "./text-field";

const meta = {
  title: "Atoms/TextField",
  component: TextField,
  args: { isInvalid: false, children: null },
  render: (args) => (
    <TextField {...args}>
      <Label>Email address</Label>
      <Input name="email" type="email" placeholder="name@example.com" />
      <Description>We will send workspace updates here.</Description>
      <FieldError>Enter a valid email address.</FieldError>
    </TextField>
  ),
} satisfies Meta<typeof TextField>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {};
export const Invalid: Story = {
  args: { isInvalid: true },
};
export const Disabled: Story = { args: { isDisabled: true } };

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("text-field"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
