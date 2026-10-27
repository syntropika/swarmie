import type { Meta, StoryObj } from "@storybook/react-vite";

import {
  ApplicationSkeleton,
  ContentSkeleton,
  ConversationSkeleton,
  LoadingRows,
  MessageSkeleton,
  NavigationSkeleton,
  SettingsSkeleton,
} from "./loading";

const meta = { title: "Patterns/Loading", parameters: { layout: "fullscreen" } } satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;
export const Application: Story = { render: () => <ApplicationSkeleton /> };
export const Conversation: Story = {
  render: () => (
    <div className="h-[600px]">
      <ConversationSkeleton />
    </div>
  ),
};
export const Navigation: Story = {
  render: () => (
    <div className="h-[600px] w-[220px]">
      <NavigationSkeleton />
    </div>
  ),
};
export const Settings: Story = {
  render: () => (
    <div className="h-[600px]">
      <SettingsSkeleton />
    </div>
  ),
};
export const Lists: Story = {
  render: () => (
    <div className="max-w-[720px] p-6">
      <LoadingRows label="Loading plugins" rows={5} detail />
    </div>
  ),
};
export const Messages: Story = {
  render: () => (
    <div className="max-w-[720px] p-6">
      <MessageSkeleton />
    </div>
  ),
};
export const Document: Story = {
  render: () => (
    <div className="max-w-[720px] p-6">
      <ContentSkeleton />
    </div>
  ),
};
