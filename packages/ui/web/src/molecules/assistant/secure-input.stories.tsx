import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, fn, userEvent, within } from "storybook/test";

import { Button } from "../../atoms/button";
import { SecureInput } from "./secure-input";
import type {
  SecureInputLabels,
  SecureInputPreset,
  SecureInputState,
  SecureInputValues,
} from "./secure-input";

const labels: SecureInputLabels = {
  continue: "Continue",
  dismiss: "Dismiss",
  openScreen: "Open on screen",
  showPassword: "Show password",
  hidePassword: "Hide password",
  digit: (position, count) => `Code, digit ${position} of ${count}`,
  expiryPlaceholder: "MM/YY",
  statuses: {
    sending: { label: "Sending", description: "Sending the requested details." },
    filled: { label: "Filled", description: "The details were filled into the page." },
    failed: { label: "Not filled", description: "The page changed. Retry from the current page." },
    dismissed: { label: "Dismissed", description: "Dismissed without sending." },
  },
};

const fieldLabels = {
  secret: { preset: "secret", fieldLabels: { code: "SMS code" } },
  login: { preset: "login", fieldLabels: { email: "Email", password: "Password" } },
  otp: { preset: "otp", fieldLabels: { code: "Authenticator code" } },
  pin: { preset: "pin", fieldLabels: { pin: "PIN" } },
  card: { preset: "card", fieldLabels: { number: "Card number", expiry: "Expiry", cvc: "CVC" } },
} as const;

function Example({
  preset = "login",
  initialState = "form",
}: {
  preset?: SecureInputPreset;
  initialState?: SecureInputState;
}) {
  const [values, setValues] = useState<SecureInputValues>({});
  const [state, setState] = useState<SecureInputState>(initialState);
  const [opened, setOpened] = useState(false);
  return (
    <div className="max-w-md space-y-4 p-6">
      <SecureInput
        {...fieldLabels[preset]}
        title="Requested details"
        description="Synthetic data for this component demonstration."
        values={values}
        onValueChange={setValues}
        state={state}
        labels={labels}
        onContinue={() => {
          setValues({});
          setState("filled");
        }}
        onDismiss={() => {
          setValues({});
          setState("dismissed");
        }}
        onOpenScreen={() => {
          setOpened(true);
        }}
      />
      {opened ? <output className="text-ui">Screen action requested</output> : null}
      {state === "form" ? null : (
        <Button
          variant="secondary"
          size="sm"
          onClick={() => {
            setState("form");
          }}
        >
          Reset example
        </Button>
      )}
    </div>
  );
}

const meta = {
  title: "Assistant/Approvals & forms/SecureInput",
  component: SecureInput,
  args: {
    ...fieldLabels.login,
    title: "Requested details",
    description: "Synthetic example",
    values: {},
    onValueChange: fn<() => void>(),
    state: "form",
    labels,
    onContinue: fn<() => void>(),
    onDismiss: fn<() => void>(),
  },
  parameters: {
    docs: {
      description: {
        component:
          "The AICSS five-preset form: login and password visibility, SMS entry, segmented OTP and PIN, formatted card details, and controlled result banners. Values and lifecycle remain with the host; no artificial transmission or success is introduced.",
      },
    },
  },
  render: () => <Example />,
} satisfies Meta<typeof SecureInput>;
export default meta;
type Story = StoryObj<typeof meta>;

async function expectCodeRow(canvasElement: HTMLElement, count: number) {
  const cells = within(canvasElement).getAllByRole("textbox");
  await expect(cells).toHaveLength(count);
  const first = cells[0]?.getBoundingClientRect();
  if (!first) {
    throw new Error("The code entry must render its digit cells.");
  }
  await Promise.all(
    cells.flatMap((cell) => {
      const rect = cell.getBoundingClientRect();
      return [
        expect(rect.width).toBeCloseTo(rect.height, 1),
        expect(rect.width).toBeLessThan(64),
        expect(rect.top).toBeCloseTo(first.top, 1),
      ];
    }),
  );
}

export const Login: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(canvas.getByLabelText("Email", { exact: false }), "alex@example.com");
    await userEvent.type(canvas.getByLabelText("Password", { exact: true }), "example-secret");
    await userEvent.click(canvas.getByRole("button", { name: "Show password" }));
    await expect(canvas.getByLabelText("Password", { exact: true })).toHaveAttribute(
      "type",
      "text",
    );
    await userEvent.click(canvas.getByRole("button", { name: "Hide password" }));
    await expect(canvas.getByLabelText("Password", { exact: true })).toHaveAttribute(
      "type",
      "password",
    );
    await userEvent.click(canvas.getByRole("button", { name: "Continue" }));
    await expect(canvas.getByText("The details were filled into the page.")).toBeVisible();
  },
};

export const SMS: Story = { render: () => <Example preset="secret" /> };
export const OTP: Story = {
  render: () => <Example preset="otp" />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expectCodeRow(canvasElement, 6);
    await userEvent.click(canvas.getByLabelText("Code, digit 1 of 6"));
    await userEvent.paste("123456");
    await expect(canvas.getByLabelText("Code, digit 6 of 6")).toHaveValue("6");
    await expect(canvas.getByRole("button", { name: "Continue" })).toBeEnabled();
    await userEvent.keyboard("{Backspace}");
    await expect(canvas.getByRole("button", { name: "Continue" })).toBeDisabled();
  },
};
export const PIN: Story = {
  render: () => <Example preset="pin" />,
  play: async ({ canvasElement }) => {
    await expectCodeRow(canvasElement, 4);
  },
};
export const Card: Story = { render: () => <Example preset="card" /> };
export const Sending: Story = { render: () => <Example initialState="sending" /> };
export const Failed: Story = { render: () => <Example initialState="failed" /> };
export const Dismissed: Story = { render: () => <Example initialState="dismissed" /> };
