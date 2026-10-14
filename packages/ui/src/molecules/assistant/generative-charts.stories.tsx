import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { GenerativeExample } from "./examples/generative-recipes";

const meta = {
  title: "Assistant/Content & data/Generative charts",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned generative charts presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const GenerativeChartArea: Story = {
  name: "Weekly active users",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-chart-area" />
    </ExampleFrame>
  ),
};

export const GenerativeChartLine: Story = {
  name: "Response time",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-chart-line" />
    </ExampleFrame>
  ),
};

export const GenerativeChartBars: Story = {
  name: "Support tickets",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-chart-bars" />
    </ExampleFrame>
  ),
};
