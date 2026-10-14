import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, userEvent, within } from "storybook/test";

import { Button } from "../../atoms/button";
import { ComposerThreadExample } from "./examples/composer-thread";
import { ExampleFrame } from "./examples/fixtures";
import { VoiceOrb as VoiceOrbView, VoiceConversation as VoiceConversationView } from "./voice";
import type { VoiceState } from "./voice";

const meta = {
  title: "Assistant/Voice/Conversation",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned React presentation contracts adapted from the Assistant UI voice concepts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const VoiceConversation: Story = {
  name: "Voice conversation",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="voice-conversation" />
    </ExampleFrame>
  ),
};

export const ReadAloud: Story = {
  name: "Read aloud",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="read-aloud" />
    </ExampleFrame>
  ),
};

/** Synthetic call transitions exercise the host-controlled voice contract. */
function VoiceLifecycleExample() {
  const [state, setState] = useState<VoiceState>("disconnected");
  const [muted, setMuted] = useState(false);
  if (state === "disconnected") {
    return (
      <VoiceOrbView
        state={state}
        label="Disconnected"
        connectLabel="Connect voice"
        onConnect={() => {
          setState("listening");
        }}
      />
    );
  }
  return (
    <div className="space-y-3">
      <VoiceConversationView
        state={state}
        amplitude={state === "speaking" ? 0.8 : 0.35}
        label={state}
        hint={state === "thinking" ? "Preparing the response" : "Voice session active"}
        transcript={null}
        turns={[
          { id: "question", role: "user", roleLabel: "You", text: "Read the summary." },
          {
            id: "answer",
            role: "assistant",
            roleLabel: "Assistant",
            text: "The checks completed.",
          },
        ]}
        muted={muted}
        muteLabel={muted ? "Unmute" : "Mute"}
        endLabel="End voice"
        onMuteChange={setMuted}
        onEnd={() => {
          setState("disconnected");
        }}
        onInterrupt={() => {
          setState("listening");
        }}
        interruptLabel="Interrupt reply"
      />
      <Button
        variant="ghost"
        onClick={() => {
          setState("thinking");
        }}
      >
        Think
      </Button>
      <Button
        variant="ghost"
        onClick={() => {
          setState("speaking");
        }}
      >
        Speak
      </Button>
    </div>
  );
}

export const VoiceLifecycle: Story = {
  render: () => (
    <ExampleFrame>
      <VoiceLifecycleExample />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Connect voice" }));
    await userEvent.click(canvas.getByRole("button", { name: "Think" }));
    await expect(canvas.getByText("Preparing the response")).toBeVisible();
    await userEvent.click(canvas.getByRole("button", { name: "Speak" }));
    await userEvent.click(canvas.getByRole("button", { name: "Interrupt reply" }));
    await expect(canvas.queryByRole("button", { name: "Interrupt reply" })).not.toBeInTheDocument();
    await userEvent.click(canvas.getByRole("button", { name: "Mute" }));
    await expect(canvas.getByRole("button", { name: "Unmute" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await userEvent.click(canvas.getByRole("button", { name: "End voice" }));
    await expect(canvas.getByRole("button", { name: "Connect voice" })).toBeVisible();
  },
};
