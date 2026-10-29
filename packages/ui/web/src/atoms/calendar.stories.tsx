import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/calendar";

const meta = {
  title: "Atoms/Calendar",
  parameters: {
    docs: {
      description: {
        component:
          "[Official HeroUI example](https://heroui.com/en/docs/react/components/calendar). Uses the upstream compound API with the shared Swarmie theme.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  parameters: { docs: { source: { code: officialExampleSource("calendar"), language: "tsx" } } },
  render: () => (
    <div className="max-w-full min-w-0 overflow-x-auto">
      <HeroUIDocumentationExample />
    </div>
  ),
};
