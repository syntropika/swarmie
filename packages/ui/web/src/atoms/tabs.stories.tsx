import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/tabs";
import { Tabs } from "./tabs";

const meta = {
  title: "Atoms/Tabs",
  parameters: {
    docs: {
      description: {
        component:
          "[Official HeroUI example](https://heroui.com/en/docs/react/components/tabs). Uses the upstream compound API with the shared Swarmie theme.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  parameters: { docs: { source: { code: officialExampleSource("tabs"), language: "tsx" } } },
  render: () => <HeroUIDocumentationExample />,
};

export const Categories: Story = {
  render: () => (
    <Tabs defaultSelectedKey="Plugins">
      <Tabs.ListContainer>
        <Tabs.List aria-label="Plugin categories">
          {[
            { name: "Plugins", count: 5 },
            { name: "Apps", count: 9 },
            { name: "MCPs", count: 6 },
            { name: "Skills", count: 37 },
            { name: "Marketplace", count: 2 },
          ].map(({ name, count }) => (
            <Tabs.Tab key={name} id={name}>
              {name}
              <span className="ml-1 text-muted">{count}</span>
              <Tabs.Indicator />
            </Tabs.Tab>
          ))}
        </Tabs.List>
      </Tabs.ListContainer>
    </Tabs>
  ),
};
