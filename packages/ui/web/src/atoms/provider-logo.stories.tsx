import type { Meta, StoryObj } from "@storybook/react-vite";

import { ProviderLogo } from "./provider-logo";

const meta = {
  title: "Atoms/ProviderLogo",
  component: ProviderLogo,
  args: { providerId: "openai" },
} satisfies Meta<typeof ProviderLogo>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Catalog: Story = {
  render: () => (
    <div className="grid gap-4">
      {(["light", "dark"] as const).map((theme) => (
        <ul key={theme} data-theme={theme} className="flex flex-wrap gap-4 bg-canvas p-4 text-ink">
          {[
            "openai",
            "claude",
            "gemini",
            "kimi",
            "alibaba",
            "github-copilot",
            "local",
            "unknown",
          ].map((providerId) => (
            <li key={providerId} className="flex items-center gap-2 text-caption">
              <ProviderLogo providerId={providerId} />
              <span>{providerId}</span>
            </li>
          ))}
        </ul>
      ))}
    </div>
  ),
};
