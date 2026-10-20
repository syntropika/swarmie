import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/textarea";
import { TextArea } from "../index";

const meta = {
  title: "Atoms/TextArea",
  parameters: { layout: "padded" },
  tags: ["autodocs"],
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const States: Story = {
  render: () => (
    <div className="grid max-w-lg gap-4">
      <label className="grid gap-2" htmlFor="textarea-field-1">
        Instructions
        <TextArea id="textarea-field-1" defaultValue="Summarize the requested changes." />
      </label>
      <label className="grid gap-2" htmlFor="textarea-field-2">
        Unavailable instructions
        <TextArea id="textarea-field-2" disabled defaultValue="Saving instructions" />
      </label>
    </div>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("textarea"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
