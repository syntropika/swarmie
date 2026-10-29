import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ChromeButton } from "../atoms/chrome-button";
import { Icon } from "../atoms/icon";
import { NavigationShell } from "./navigation-shell";

function ExampleNavigation({ collapsible = true }: { collapsible?: boolean }) {
  const [section, setSection] = useState<"conversations" | "settings">("conversations");
  const [draft, setDraft] = useState(false);
  return (
    <NavigationShell
      collapsible={collapsible}
      labels={{
        navigation: "Example navigation",
        openNavigation: "Open navigation",
        closeNavigation: "Close navigation",
        collapseNavigation: "Collapse navigation",
        pinNavigation: "Pin navigation",
        resizeNavigation: "Resize navigation",
      }}
      renderRail={({ closeNavigation, revealSidebar, previewNavigation, leavePreview }) => (
        <nav
          aria-label="Example rail"
          className="flex w-(--spacing-shell-rail) shrink-0 flex-col items-center gap-(--spacing-chrome-rail-gap) py-3"
        >
          <ChromeButton size="rail" aria-label="Workspace" onClick={revealSidebar}>
            <Icon name="workspaces" />
          </ChromeButton>
          <ChromeButton
            size="rail"
            aria-label="Home"
            data-navigation-home="true"
            onClick={() => {
              setDraft(true);
              setSection("conversations");
              closeNavigation();
            }}
            onPointerEnter={(event) => {
              if (event.pointerType === "mouse") {
                previewNavigation();
              }
            }}
            onPointerLeave={leavePreview}
            onKeyDown={(event) => {
              if (event.key === "ArrowRight") {
                event.preventDefault();
                previewNavigation({ focus: true });
              }
            }}
          >
            <Icon name="home" />
          </ChromeButton>
          <ChromeButton
            size="rail"
            aria-label="Conversations"
            aria-current={section === "conversations" ? "page" : false}
            onClick={() => {
              setSection("conversations");
              setDraft(false);
              closeNavigation();
            }}
          >
            <Icon name="chat" />
          </ChromeButton>
          <ChromeButton
            size="rail"
            aria-label="Settings"
            aria-current={section === "settings" ? "page" : false}
            onClick={() => {
              setSection("settings");
              closeNavigation();
            }}
          >
            <Icon name="settings" />
          </ChromeButton>
        </nav>
      )}
      renderSidebar={({ navigationControl, closeNavigation }) => (
        <aside className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-navigation-surface px-2 pb-2">
          <div className="flex min-h-14 items-center">
            <h2 className="min-w-0 flex-1 px-3 text-ui font-medium">
              {section === "settings" ? "Settings" : "Example workspace"}
            </h2>
            {navigationControl}
          </div>
          <ChromeButton size="navigation" onClick={closeNavigation}>
            {section === "settings" ? "Appearance" : "Example conversation"}
          </ChromeButton>
        </aside>
      )}
    >
      <main className="min-h-0 flex-1 overflow-y-auto px-6 pt-20 pb-8 md:pt-10">
        <div className="mx-auto max-w-3xl">
          <h1 className="text-title font-medium">
            {draft ? "New conversation" : "Working canvas"}
          </h1>
          <p className="mt-4 max-w-[65ch] leading-relaxed text-text-muted">
            This illustrative navigation uses the same rail and responsive sidebar for two
            capabilities. The consumer controls its actions and canvas content.
          </p>
        </div>
      </main>
    </NavigationShell>
  );
}

const meta = {
  title: "Molecules/Navigation Shell",
  component: ExampleNavigation,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ExampleNavigation>;

export default meta;
type Story = StoryObj<typeof meta>;

export const SharedCapabilityLayout: Story = {};

/** Pinned settings navigation permits resizing but cannot collapse at its minimum width. */
export const FixedSidebar: Story = {
  args: { collapsible: false },
  play: async ({ canvasElement }) => {
    if (window.innerWidth < 768) {
      return;
    }
    const canvas = within(canvasElement);
    await expect(canvas.queryByRole("button", { name: "Collapse navigation" })).toBeNull();
    const separator = canvas.getByRole("separator", { name: "Resize navigation" });
    separator.focus();
    await userEvent.keyboard("{End}");
    await waitFor(async () => {
      await expect(separator).toHaveAttribute("aria-valuenow", "513");
    });
    await userEvent.keyboard("{Home}{ArrowLeft}");
    await waitFor(async () => {
      await expect(separator).toHaveAttribute("aria-valuenow", "283");
    });
    await expect(separator).toBeVisible();
  },
};

export const ResponsiveFocus: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    if (window.innerWidth >= 768) {
      await userEvent.click(canvas.getByRole("button", { name: "Collapse navigation" }));
      const home = canvas.getByRole("button", { name: "Home" });
      await expect(home).toHaveFocus();
      await userEvent.keyboard("{ArrowRight}");
      const temporary = await canvas.findByRole("dialog", { name: "Example navigation" });
      await expect(temporary).toBeVisible();
      await userEvent.keyboard("{Escape}");
      await expect(home).toHaveFocus();
      await userEvent.hover(home);
      await userEvent.click(within(temporary).getByRole("button", { name: "Pin navigation" }));
      await expect(canvas.getByRole("button", { name: "Collapse navigation" })).toBeVisible();
      await userEvent.click(home);
      await expect(canvas.getByRole("heading", { name: "New conversation" })).toBeVisible();
      await expect(canvas.getByRole("button", { name: "Collapse navigation" })).toBeVisible();
    } else {
      const opener = canvas.getByRole("button", { name: "Open navigation" });
      await userEvent.click(opener);
      const dialog = await canvas.findByRole("dialog", { name: "Example navigation" });
      await userEvent.click(within(dialog).getByRole("button", { name: "Close navigation" }));
      await expect(opener).toHaveFocus();
    }
  },
};

/** The adjustable splitter exposes total rail/sidebar bounds to pointer and keyboard users. */
export const KeyboardResizeBounds: Story = {
  play: async ({ canvasElement }) => {
    if (window.innerWidth < 768) {
      return;
    }
    const canvas = within(canvasElement);
    const separator = canvas.getByRole("separator", { name: "Resize navigation" });
    separator.focus();
    await userEvent.keyboard("{End}");
    await waitFor(async () => {
      await expect(separator).toHaveAttribute("aria-valuenow", "513");
    });
    await userEvent.keyboard("{Home}");
    await waitFor(async () => {
      await expect(separator).toHaveAttribute("aria-valuenow", "283");
    });
    await userEvent.keyboard("{ArrowRight}");
    await waitFor(async () => {
      await expect(separator).toHaveAttribute("aria-valuenow", "291");
    });
  },
};
