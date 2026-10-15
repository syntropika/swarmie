import type { Meta, StoryObj } from "@storybook/react-vite";

import { Field } from "../molecules/field";
import { Button } from "./button";
import { Input } from "./input";

const meta = {
  title: "Atoms/Input",
  component: Input,
  args: { placeholder: "name@example.com", type: "email", autoComplete: "email" },
  render: (args) => (
    <Field id="email-input" label="Email address" className="max-w-sm">
      {(control) => <Input {...args} {...control} />}
    </Field>
  ),
} satisfies Meta<typeof Input>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const EntryField: Story = {
  args: { controlSize: "large" },
};

export const ReadOnly: Story = { args: { defaultValue: "alex@example.com", readOnly: true } };

export const Disabled: Story = { args: { defaultValue: "alex@example.com", disabled: true } };

export const TrailingAction: Story = {
  render: () => (
    <Field id="search-input" label="Search" className="max-w-sm">
      {(control) => (
        <div className="relative">
          <Input {...control} adornment="trailing" type="search" />
          <Button
            variant="ghost"
            size="icon"
            className="absolute top-1/2 right-0 -translate-y-1/2"
            aria-label="Submit search"
          >
            ↵
          </Button>
        </div>
      )}
    </Field>
  ),
};
