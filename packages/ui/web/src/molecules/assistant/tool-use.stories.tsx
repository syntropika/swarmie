import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { ToolsKnowledgeExample, ToolUIExample } from "./examples/tools-knowledge";

const meta = {
  title: "Assistant/Tools/Calls & results",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned calls & results presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const ToolCall: Story = {
  name: "Tool call",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="tool-call" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByText("Reading session contracts", { exact: true }));
    await expect(canvas.getByText('{"path":"docs/agents/frontend.md"}')).toBeVisible();
    await expect(
      canvas.queryByText("The frontend guide requires controlled shared components."),
    ).not.toBeInTheDocument();
    await userEvent.click(canvas.getByRole("button", { name: "Finish tool" }));
    await expect(
      canvas.getByText("The frontend guide requires controlled shared components."),
    ).toBeVisible();
    await waitFor(async () => {
      await expect(canvas.getByText("Read session contracts", { exact: true })).toBeVisible();
    });
    await expect(
      canvas.getByText("Read session contracts", { exact: true }).closest("details"),
    ).toHaveAttribute("open");
  },
};

export const ToolTimeline: Story = {
  name: "Tool timeline",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="tool-timeline" />
    </ExampleFrame>
  ),
};

export const ToolError: Story = {
  name: "Tool failure",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="tool-error" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Retry this tool" }));
    await waitFor(async () => {
      await expect(canvas.getByText("Retrying search")).toBeVisible();
    });
    await expect(canvas.getByRole("button", { name: "Skip tool" })).toBeDisabled();
    await userEvent.click(canvas.getByRole("button", { name: "Simulate failed retry" }));
    await expect(canvas.getByRole("button", { name: "Retry this tool" })).toBeEnabled();
  },
};

export const ToolUI: Story = {
  name: "Tool UI renderer",
  parameters: {
    docs: {
      description: {
        story:
          "A synthetic weather-tool invocation demonstrates argument streaming, running, rich results, and error recovery. The host owns args, result, execution, and toolkit registration; the shared renderer performs no requests.",
      },
    },
  },
  render: () => (
    <ExampleFrame>
      <ToolUIExample />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(
      canvas.queryByRole("region", { name: "Weather in San Francisco" }),
    ).not.toBeInTheDocument();
    await userEvent.click(canvas.getByRole("button", { name: "Start tool" }));
    await expect(canvas.getByText("Checking weather in San Francisco…")).toBeVisible();
    await userEvent.click(canvas.getByRole("button", { name: "Resolve forecast" }));
    await expect(canvas.getByRole("region", { name: "Weather in San Francisco" })).toBeVisible();
    await expect(canvas.getByText("72°F")).toBeVisible();
    await userEvent.click(canvas.getByRole("button", { name: "Fail tool" }));
    await expect(
      canvas.queryByRole("region", { name: "Weather in San Francisco" }),
    ).not.toBeInTheDocument();
    await userEvent.click(canvas.getByRole("button", { name: "Retry forecast" }));
    await expect(canvas.getByText("Checking weather in San Francisco…")).toBeVisible();
  },
};

export const ToolUIResult: Story = {
  name: "Tool UI result",
  render: () => (
    <ExampleFrame>
      <ToolUIExample initialPhase="completed" />
    </ExampleFrame>
  ),
};

export const ToolUIError: Story = {
  name: "Tool UI error",
  render: () => (
    <ExampleFrame>
      <ToolUIExample initialPhase="failed" />
    </ExampleFrame>
  ),
};
