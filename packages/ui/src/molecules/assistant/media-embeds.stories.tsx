import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { StructuredAgentExample } from "./examples/structured-agents";

const meta = {
  title: "Assistant/Content & data/Media & embeds",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned media & embeds presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const MediaPlayer: Story = {
  name: "Media player",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="media-player" />
    </ExampleFrame>
  ),
};

export const WebPreview: Story = {
  name: "Web preview",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="web-preview" />
    </ExampleFrame>
  ),
};
