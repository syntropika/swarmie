import type { Meta, StoryObj } from "@storybook/react-vite";

import { ComposerThreadExample } from "./examples/composer-thread";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Composer/MobileComposer",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned mobilecomposer presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const MobileComposer: Story = {
  name: "Mobile composer",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="mobile-composer" />
    </ExampleFrame>
  ),
};
