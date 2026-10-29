import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/popover";
import { Button } from "./button";
import { Label } from "./label";
import { Popover } from "./popover";
import { Slider } from "./slider";

const meta = {
  title: "Atoms/Popover",
  component: Popover,
  args: { children: null },
} satisfies Meta<typeof Popover>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  render: () => (
    <Popover>
      <Button variant="secondary">Show details</Button>
      <Popover.Content>
        <Popover.Dialog aria-label="Project details">
          <Popover.Heading>Project details</Popover.Heading>
          <p>A shared project for the team.</p>
        </Popover.Dialog>
      </Popover.Content>
    </Popover>
  ),
};

export const SliderContent: Story = {
  render: () => (
    <Popover>
      <Button variant="tertiary">Thinking effort</Button>
      <Popover.Content placement="top end">
        <Popover.Dialog aria-label="Thinking effort">
          <Slider minValue={0} maxValue={2} step={1} defaultValue={1}>
            <Label>Thinking effort</Label>
            <Slider.Output>
              {({ state }) => ["Less", "Balanced", "More"][state.values[0] ?? 0]}
            </Slider.Output>
            <Slider.Track>
              <Slider.Fill />
              <Slider.Thumb />
            </Slider.Track>
          </Slider>
        </Popover.Dialog>
      </Popover.Content>
    </Popover>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("popover"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
