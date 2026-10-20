import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/date-field";

const meta = {
  title: "Atoms/DateField",
  parameters: {
    docs: {
      description: {
        component:
          "[Official HeroUI example](https://heroui.com/en/docs/react/components/date-field). Uses the upstream compound API with the shared Swarmie theme.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  parameters: { docs: { source: { code: officialExampleSource("date-field"), language: "tsx" } } },
  render: () => <HeroUIDocumentationExample />,
};
