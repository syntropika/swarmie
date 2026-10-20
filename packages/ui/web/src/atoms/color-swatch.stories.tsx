import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/color-swatch";

const meta = {
  title: "Atoms/ColorSwatch",
  parameters: {
    docs: {
      description: {
        component:
          "[Official HeroUI example](https://heroui.com/en/docs/react/components/color-swatch). Uses the upstream compound API with the shared Swarmie theme.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  parameters: {
    docs: { source: { code: officialExampleSource("color-swatch"), language: "tsx" } },
  },
  render: () => <HeroUIDocumentationExample />,
};
