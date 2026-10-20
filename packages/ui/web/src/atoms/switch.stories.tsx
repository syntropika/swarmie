import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/switch";
import { Switch } from "./switch";

const meta = { title: "Atoms/Switch", component: Switch, args: { children: null } } satisfies Meta<
  typeof Switch
>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  render: () => (
    <Switch>
      <Switch.Content>
        <Switch.Control>
          <Switch.Thumb />
        </Switch.Control>
        Notify me about updates
      </Switch.Content>
    </Switch>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("switch"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
