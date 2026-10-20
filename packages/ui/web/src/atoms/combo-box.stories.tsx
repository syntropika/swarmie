import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/combo-box";
import { ComboBox } from "./combo-box";
import { Input } from "./input";
import { ListBox } from "./select";

const meta = {
  title: "Atoms/ComboBox",
  component: ComboBox,
  args: { children: null },
} satisfies Meta<typeof ComboBox>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  render: () => (
    <ComboBox aria-label="Project" className="max-w-sm">
      <ComboBox.InputGroup>
        <Input placeholder="Choose a project" />
        <ComboBox.Trigger />
      </ComboBox.InputGroup>
      <ComboBox.Popover>
        <ListBox>
          <ListBox.Item id="web" textValue="Website">
            Website
            <ListBox.ItemIndicator />
          </ListBox.Item>
          <ListBox.Item id="desktop" textValue="Desktop">
            Desktop
            <ListBox.ItemIndicator />
          </ListBox.Item>
        </ListBox>
      </ComboBox.Popover>
    </ComboBox>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("combo-box"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
