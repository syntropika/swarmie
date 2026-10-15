import type { Meta, StoryObj } from "@storybook/react-vite";
import type { ComponentProps } from "react";
import { useState } from "react";
import { expect, fn, userEvent, within } from "storybook/test";

import {
  NativeButton,
  NativeCodeField,
  NativeField,
  NativeIconAction,
  NativeNotice,
  NativeTextAction,
} from "./controls";

type ButtonStoryProps = Omit<ComponentProps<typeof NativeButton>, "onPress"> & {
  onPress: () => void;
};

const meta = {
  title: "Native/Controls",
  component: NativeButton,
  args: { label: "Continue", onPress: fn<() => void>() },
  // Native events carry host objects; log the action without serializing renderer internals.
  render: ({ onPress, ...args }) => (
    <NativeButton
      {...args}
      onPress={() => {
        onPress();
      }}
    />
  ),
  decorators: [
    (Story) => (
      <div className="max-w-native-content bg-native-canvas p-6">
        <Story />
      </div>
    ),
  ],
} satisfies Meta<ButtonStoryProps>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {
  play: async ({ canvasElement, args }) => {
    await userEvent.click(await within(canvasElement).findByRole("button", { name: "Continue" }));
    await expect(args.onPress).toHaveBeenCalledOnce();
  },
};
export const Secondary: Story = { args: { label: "Use a password", tone: "secondary" } };
export const Pending: Story = { args: { label: "Signing in…", loading: true } };
export const Disabled: Story = { args: { disabled: true } };

function FieldExamples() {
  const [email, setEmail] = useState("");
  const [code, setCode] = useState("");
  return (
    <div className="flex flex-col gap-6">
      <NativeField
        label="Email address"
        value={email}
        onChangeText={setEmail}
        autoComplete="email"
        placeholder="you@example.com"
      />
      <NativeCodeField value={code} onChangeText={setCode} />
      <NativeNotice>That code didn&apos;t work. Check the email and try again.</NativeNotice>
      <NativeTextAction
        label="Resend code"
        onPress={() => {
          setCode("");
        }}
      />
    </div>
  );
}

export const Fields: Story = {
  render: () => <FieldExamples />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(
      await canvas.findByRole("textbox", { name: "Email address" }),
      "sam@example.com",
    );
    await expect(canvas.getByRole("textbox", { name: "Email address" })).toHaveValue(
      "sam@example.com",
    );
    await userEvent.type(canvas.getByLabelText("Six-digit verification code"), "123456");
    await expect(canvas.getByLabelText("Six-digit verification code")).toHaveValue("123456");
  },
};

export const IconAction: Story = {
  render: ({ onPress }) => (
    <NativeIconAction
      accessibilityLabel="Show password"
      onPress={() => {
        onPress();
      }}
    >
      <svg
        width="28"
        height="28"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
        className="text-native-muted"
      >
        <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z" />
        <circle cx="12" cy="12" r="3" />
      </svg>
    </NativeIconAction>
  ),
  play: async ({ canvasElement, args }) => {
    await userEvent.click(
      await within(canvasElement).findByRole("button", { name: "Show password" }),
    );
    await expect(args.onPress).toHaveBeenCalledOnce();
  },
};
