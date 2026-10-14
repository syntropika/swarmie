import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, waitFor, within } from "storybook/test";

import { Button } from "../../atoms/button";
import { ExampleFrame } from "./examples/fixtures";
import { StreamingTextReveal } from "./streaming-text-reveal";

const meta = {
  title: "Assistant/Content & data/StreamingTextReveal",
  component: StreamingTextReveal,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof StreamingTextReveal>;
export default meta;
type Story = StoryObj;

function StreamExample() {
  const [run, setRun] = useState(0);
  return (
    <ExampleFrame>
      <StreamingTextReveal
        key={run}
        text="The components share Swarmie’s typography and color tokens. Each action has an explicit owner, and the reading surface stays comfortable as the answer arrives."
      />
      <Button
        variant="secondary"
        onClick={() => {
          setRun(run + 1);
        }}
      >
        Replay
      </Button>
    </ExampleFrame>
  );
}

export const Default: Story = {
  render: () => <StreamExample />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await waitFor(
      async () => {
        await expect(
          canvas.getByText(/reading surface stays comfortable as the answer arrives/u),
        ).toHaveAttribute("aria-busy", "false");
      },
      { timeout: 5000 },
    );
  },
};
