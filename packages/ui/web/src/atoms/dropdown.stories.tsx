import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/dropdown";
import { Button } from "./button";
import { Dropdown } from "./dropdown";

const meta = {
  title: "Atoms/Dropdown",
  component: Dropdown,
  args: { children: null },
} satisfies Meta<typeof Dropdown>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  render: () => (
    <Dropdown>
      <Button variant="secondary">Project actions</Button>
      <Dropdown.Popover>
        <Dropdown.Menu aria-label="Project actions">
          <Dropdown.Item id="open" textValue="Open project">
            Open project
          </Dropdown.Item>
          <Dropdown.Item id="remove" textValue="Remove project" variant="danger">
            Remove project
          </Dropdown.Item>
        </Dropdown.Menu>
      </Dropdown.Popover>
    </Dropdown>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("dropdown"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
