import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/close-button";
import { CloseButton } from "./close-button";

const meta = {
  title: "Atoms/CloseButton",
  parameters: {
    docs: {
      description: {
        component:
          "[Official HeroUI example](https://heroui.com/en/docs/react/components/close-button). Uses the upstream compound API with the shared Swarmie theme.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  parameters: {
    docs: { source: { code: officialExampleSource("close-button"), language: "tsx" } },
  },
  render: () => <HeroUIDocumentationExample />,
};

export const States: Story = {
  render: () => (
    <div className="flex items-center gap-3">
      <CloseButton aria-label="Close panel" />
      <CloseButton isDisabled aria-label="Unavailable close" />
    </div>
  ),
};
