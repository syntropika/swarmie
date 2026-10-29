import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/toggle-button";
import { Icon } from "./icon";
import { ToggleButton } from "./toggle-button";

const meta = {
  title: "Atoms/ToggleButton",
  parameters: {
    docs: {
      description: {
        component:
          "[Official HeroUI example](https://heroui.com/en/docs/react/components/toggle-button). Uses the upstream compound API with the shared Swarmie theme.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  parameters: {
    docs: { source: { code: officialExampleSource("toggle-button"), language: "tsx" } },
  },
  render: () => <HeroUIDocumentationExample />,
};

export const States: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-3">
      <ToggleButton>
        <Icon name="pin" />
        Pin
      </ToggleButton>
      <ToggleButton defaultSelected>
        <Icon name="pin" />
        Pinned
      </ToggleButton>
      <ToggleButton isDisabled>
        <Icon name="pin" />
        Unavailable
      </ToggleButton>
      <ToggleButton isDisabled isSelected>
        <Icon name="pin" />
        Pinned unavailable
      </ToggleButton>
      <ToggleButton isIconOnly aria-label="Pin">
        <Icon name="pin" />
      </ToggleButton>
    </div>
  ),
};
