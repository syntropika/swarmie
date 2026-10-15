import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { ToolsKnowledgeExample } from "./examples/tools-knowledge";

const meta = {
  title: "Assistant/Content & data/Maps",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned maps presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const MapAnswer: Story = {
  name: "Map",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="map-answer" />
    </ExampleFrame>
  ),
};

export const GeoMap: Story = {
  name: "Geo map",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="geo-map" />
    </ExampleFrame>
  ),
};
