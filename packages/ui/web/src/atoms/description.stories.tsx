import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/description";

const meta = {
  title: "Atoms/Description",
  parameters: {
    docs: {
      description: {
        component:
          "[Official HeroUI example](https://heroui.com/en/docs/react/components/description). Uses the upstream compound API with the shared Swarmie theme.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  parameters: { docs: { source: { code: officialExampleSource("description"), language: "tsx" } } },
  render: () => <HeroUIDocumentationExample />,
};
