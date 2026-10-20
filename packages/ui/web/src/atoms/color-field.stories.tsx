import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/color-field";

const meta = {
  title: "Atoms/ColorField",
  parameters: {
    docs: {
      description: {
        component:
          "[Official HeroUI example](https://heroui.com/en/docs/react/components/color-field). Uses the upstream compound API with the shared Swarmie theme.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  parameters: { docs: { source: { code: officialExampleSource("color-field"), language: "tsx" } } },
  render: () => <HeroUIDocumentationExample />,
};
