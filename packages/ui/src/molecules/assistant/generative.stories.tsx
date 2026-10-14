import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { GenerativeExample } from "./examples/generative-recipes";

const meta = {
  title: "Assistant/Generative UI/Cards & forms",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned cards & forms presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const GenerativeStays: Story = {
  name: "Riverside Loft",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-stays" />
    </ExampleFrame>
  ),
};

export const GenerativeBooking: Story = {
  name: "Table reservation",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-booking" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(canvas.getByRole("textbox", { name: "Guest name" }), "Alex");
    await userEvent.click(canvas.getByRole("radio", { name: "Terrace" }));
    await userEvent.click(canvas.getByRole("checkbox", { name: "Send a reminder 1 hour before" }));
    await userEvent.click(canvas.getByRole("button", { name: "Confirm reservation" }));
    const receipt = canvas.getByText(/Demo action: book/u);
    await expect(receipt).toHaveTextContent("party: 2");
    await expect(receipt).toHaveTextContent("date: 2026-10-03");
    await expect(receipt).toHaveTextContent("seating: terrace");
    await expect(receipt).toHaveTextContent("reminder: true");
  },
};

export const GenerativeOrderStatus: Story = {
  name: "Order tracking",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-order-status" />
    </ExampleFrame>
  ),
};

export const GenerativeFlightTracker: Story = {
  name: "Flight tracker",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-flight-tracker" />
    </ExampleFrame>
  ),
};

export const GenerativePortfolio: Story = {
  name: "Portfolio overview",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-portfolio" />
    </ExampleFrame>
  ),
};

export const GenerativeCreateEvent: Story = {
  name: "Create event",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-create-event" />
    </ExampleFrame>
  ),
};

export const GenerativeViewEvent: Story = {
  name: "View event",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-view-event" />
    </ExampleFrame>
  ),
};

export const GenerativeWeatherCurrent: Story = {
  name: "Current weather",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-weather-current" />
    </ExampleFrame>
  ),
};

export const GenerativeRideStatus: Story = {
  name: "Ride status",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-ride-status" />
    </ExampleFrame>
  ),
};

export const GenerativeDraftEmail: Story = {
  name: "Draft email",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-draft-email" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.getByRole("textbox", { name: "From" })).toHaveAttribute("readonly");
    await userEvent.click(canvas.getByRole("button", { name: "Send demo email" }));
    await expect(canvas.getByText(/Demo action: send/u)).toHaveTextContent("to: team@example.com");
    await expect(canvas.getByText(/Demo action: send/u)).toHaveTextContent(
      "subject: Design system review",
    );
  },
};

export const GenerativeCart: Story = {
  name: "Shopping cart",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-cart" />
    </ExampleFrame>
  ),
};

export const GenerativePlaylist: Story = {
  name: "Playlist",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-playlist" />
    </ExampleFrame>
  ),
};

export const GenerativeChannelMessage: Story = {
  name: "Channel message",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-channel-message" />
    </ExampleFrame>
  ),
};

export const GenerativeReceipt: Story = {
  name: "Purchase receipt",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-receipt" />
    </ExampleFrame>
  ),
};

export const GenerativePlayerCard: Story = {
  name: "Player card",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-player-card" />
    </ExampleFrame>
  ),
};

export const GenerativeEventSession: Story = {
  name: "Conference session",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-event-session" />
    </ExampleFrame>
  ),
};

export const GenerativeNotifyConfirm: Story = {
  name: "Confirm dialog",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-notify-confirm" />
    </ExampleFrame>
  ),
};

export const GenerativeCreateTask: Story = {
  name: "Create task",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-create-task" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.clear(canvas.getByRole("textbox", { name: "Title" }));
    await userEvent.click(canvas.getByRole("button", { name: "Create demo task" }));
    await expect(canvas.queryByText(/Demo action: create/u)).not.toBeInTheDocument();
    await userEvent.type(canvas.getByRole("textbox", { name: "Title" }), "Inspect focus");
    await userEvent.click(canvas.getByRole("button", { name: "Create demo task" }));
    await expect(canvas.getByText(/Demo action: create/u)).toBeVisible();
  },
};

export const GenerativeSoftwarePurchase: Story = {
  name: "Software purchase",
  render: () => (
    <ExampleFrame>
      <GenerativeExample slug="generative-software-purchase" />
    </ExampleFrame>
  ),
};
