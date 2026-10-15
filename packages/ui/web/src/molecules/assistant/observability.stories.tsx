import type { Meta, StoryObj } from "@storybook/react-vite";
import { fn } from "storybook/test";

import { ComposerThreadExample } from "./examples/composer-thread";
import { ExampleFrame } from "./examples/fixtures";
import {
  CostMeter as CostMeterView,
  TraceWaterfall as TraceWaterfallView,
  QuotaBanner as QuotaBannerView,
} from "./observability";

const meta = {
  title: "Assistant/Observability/Usage & traces",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned React presentation contracts adapted from the Assistant UI observability concepts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const TraceWaterfall: Story = {
  name: "Trace waterfall",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="trace-waterfall" />
    </ExampleFrame>
  ),
};

export const CostMeter: Story = {
  name: "Cost meter",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="cost-meter" />
    </ExampleFrame>
  ),
};

export const QuotaBanner: Story = {
  name: "Quota banner",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="quota-banner" />
    </ExampleFrame>
  ),
};

export const RunningAndFailedSpans: Story = {
  render: () => (
    <ExampleFrame>
      <TraceWaterfallView
        label="Trace status"
        duration={1000}
        spans={[
          {
            id: "read",
            label: "Read",
            start: 0,
            duration: 240,
            depth: 0,
            detail: "Source loaded",
            state: "completed",
            statusLabel: "Completed",
          },
          {
            id: "tool",
            label: "Tool",
            start: 250,
            duration: 500,
            depth: 1,
            detail: "Waiting for a response",
            state: "running",
            statusLabel: "Running",
          },
          {
            id: "fallback",
            label: "Fallback",
            start: 800,
            duration: 180,
            depth: 1,
            detail: "Source unavailable",
            state: "failed",
            statusLabel: "Failed",
          },
        ]}
      />
    </ExampleFrame>
  ),
};
export const RunAndSessionCosts: Story = {
  render: () => (
    <ExampleFrame>
      <CostMeterView
        label="This run"
        sessionAmount={0.42}
        sessionLabel="Session"
        costs={[
          {
            id: "a",
            model: "Balanced",
            amount: 0.03,
            tokens: "1,200 total",
            inputTokens: 1000,
            outputTokens: 200,
          },
          {
            id: "b",
            model: "Fast",
            amount: 0.01,
            tokens: "500 total",
            inputTokens: 400,
            outputTokens: 100,
          },
        ]}
      />
    </ExampleFrame>
  ),
};
export const UsageWarning: Story = {
  render: () => (
    <ExampleFrame>
      <QuotaBannerView
        title="10 messages remaining"
        remaining={10}
        maximum={100}
        unit="messages used"
        usageLabel="Messages used"
        resetLabel="Resets tomorrow"
        actionLabel="Manage usage"
        onAction={fn<() => void>()}
      />
    </ExampleFrame>
  ),
};
