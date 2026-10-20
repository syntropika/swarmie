import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/list-box";

const meta = {
  title: "Atoms/ListBox",
  parameters: {
    docs: {
      description: {
        component:
          "[Official HeroUI example](https://heroui.com/en/docs/react/components/list-box). Uses the upstream compound API with the shared Swarmie theme.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  parameters: { docs: { source: { code: officialExampleSource("list-box"), language: "tsx" } } },
  render: () => <HeroUIDocumentationExample />,
};
