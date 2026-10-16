import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, fn, userEvent, waitFor, within } from "storybook/test";

import { Button } from "../atoms/button";
import { Kbd } from "../atoms/kbd";
import type { SearchDialogProps } from "./search-dialog";
import { SearchDialog } from "./search-dialog";

function SearchExample(props: SearchDialogProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const term = query.toLowerCase();
  return (
    <div className="p-4">
      <Button
        variant="secondary"
        onPress={() => {
          setOpen(true);
        }}
      >
        Search workspace
      </Button>
      {open && (
        <SearchDialog
          {...props}
          query={query}
          onQueryChange={setQuery}
          sections={props.sections.map((section) => ({
            ...section,
            items: section.items.filter((item) =>
              `${item.label} ${item.description ?? ""}`.toLowerCase().includes(term),
            ),
          }))}
          onClose={() => {
            props.onClose();
            setOpen(false);
          }}
          onAction={(id) => {
            props.onAction(id);
            setOpen(false);
          }}
        />
      )}
    </div>
  );
}

const meta = {
  title: "Molecules/SearchDialog",
  component: SearchDialog,
  parameters: {
    docs: {
      description: {
        component:
          "A shared modal search workflow. The consumer owns filtering, sections, copy and actions; the molecule owns integrated field geometry, bounded scrolling and accessible keyboard navigation. Item IDs must be unique across all sections.",
      },
    },
  },
  args: {
    title: "Search workspace",
    placeholder: "Search chats, files and commands…",
    query: "",
    onQueryChange: fn<(query: string) => void>(),
    onAction: fn<(id: string) => void>(),
    onClose: fn<() => void>(),
    emptyMessage: "No matching results.",
    sections: [
      {
        id: "pinned",
        label: "Pinned",
        items: [
          {
            id: "design",
            label: "Design system",
            description: "Review spacing and component consistency",
            metadata: "Yesterday",
          },
        ],
      },
      {
        id: "recent",
        label: "Recent chats",
        items: [
          {
            id: "keyboard",
            label: "Keyboard navigation",
            description: "Move through groups with the arrow keys",
          },
          {
            id: "release",
            label: "Prepare the release",
            description: "Packaging notes and release checklist",
          },
          {
            id: "long",
            label:
              "A long conversation title that should stay on one line even in a narrow search dialog",
            description:
              "Supporting text also truncates without pushing the row outside its container",
          },
        ],
      },
      {
        id: "commands",
        label: "Commands",
        items: [
          { id: "new", label: "New chat", metadata: <Kbd>⌘ N</Kbd> },
          { id: "settings", label: "Open settings" },
        ],
      },
    ],
  },
  render: (args) => <SearchExample {...args} />,
} satisfies Meta<typeof SearchDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

export const GroupedResults: Story = {
  play: async ({ canvasElement, args }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Search workspace" }));
    const dialog = within(
      await within(document.body).findByRole("dialog", { name: "Search workspace" }),
    );
    await userEvent.type(dialog.getByRole("combobox"), "keyboard");
    await expect(dialog.getAllByRole("option")).toHaveLength(1);
    await userEvent.keyboard("{Enter}");
    await expect(args.onAction).toHaveBeenCalledWith("keyboard");
    await expect(canvas.getByRole("button", { name: "Search workspace" })).toHaveFocus();
  },
};

export const Empty: Story = {
  args: {
    sections: [],
    emptyMessage: "No chats to search yet.",
    emptyAction: { label: "New chat", onPress: fn<() => void>() },
  },
};

export const Loading: Story = {
  args: {
    sections: [],
    emptyMessage: "Searching project files…",
    status: <output>Searching…</output>,
  },
};

export const Failure: Story = {
  args: {
    sections: [],
    emptyMessage: "Files could not be loaded.",
    status: (
      <p role="alert" className="text-danger">
        The project is unavailable. Try reopening it.
      </p>
    ),
  },
};

function ScopedSearchExample() {
  const [open, setOpen] = useState(true);
  const [scoped, setScoped] = useState(true);
  const [query, setQuery] = useState("");
  const files = [
    { id: "main", label: "src/main.ts" },
    { id: "styles", label: "src/styles.css" },
  ];
  return open ? (
    <SearchDialog
      title="Search workspace"
      inputLabel={scoped ? "Find file" : "Search workspace"}
      placeholder={scoped ? "Find a file…" : "Search files and commands…"}
      query={query}
      onQueryChange={setQuery}
      context={
        scoped
          ? {
              label: "Project files",
              onBack: () => {
                setScoped(false);
              },
            }
          : undefined
      }
      sections={[
        { id: "files", label: "Files", items: files.filter((file) => file.label.includes(query)) },
        {
          id: "commands",
          label: "Commands",
          items: scoped
            ? []
            : [{ id: "new", label: "New chat" }].filter((item) => item.label.includes(query)),
        },
      ]}
      onAction={() => {
        setOpen(false);
      }}
      onClose={() => {
        setOpen(false);
      }}
      emptyMessage="No matching results."
    />
  ) : (
    <Button
      onPress={() => {
        setOpen(true);
        setScoped(true);
      }}
    >
      Find file
    </Button>
  );
}

export const ScopedNavigation: Story = {
  render: () => <ScopedSearchExample />,
  play: async () => {
    const dialog = within(
      await within(document.body).findByRole("dialog", { name: "Search workspace" }),
    );
    await expect(dialog.getByRole("combobox", { name: "Find file" })).toHaveFocus();
    await userEvent.keyboard("{Backspace}");
    await expect(dialog.getByRole("combobox", { name: "Search workspace" })).toHaveFocus();
    await userEvent.keyboard("{Tab}");
    await expect(dialog.getByRole("option", { name: "New chat" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  },
};

export const ScrollBetweenGroups: Story = {
  args: {
    sections: [
      {
        id: "files",
        label: "Files",
        items: Array.from({ length: 40 }, (_, index) => ({
          id: `file-${index}`,
          label: `src/component-${index}.tsx`,
        })),
      },
      { id: "commands", label: "Commands", items: [{ id: "new", label: "New chat" }] },
    ],
  },
  play: async ({ canvasElement }) => {
    await userEvent.click(within(canvasElement).getByRole("button", { name: "Search workspace" }));
    const dialog = within(
      await within(document.body).findByRole("dialog", { name: "Search workspace" }),
    );
    await userEvent.keyboard("{Tab}");
    const command = dialog.getByRole("option", { name: "New chat" });
    const list = dialog.getByRole("listbox");
    await expect(command).toHaveAttribute("aria-selected", "true");
    await waitFor(async () => {
      await expect(list.scrollTop).toBeGreaterThan(0);
      await expect(command.getBoundingClientRect().bottom).toBeLessThanOrEqual(
        list.getBoundingClientRect().bottom,
      );
    });
  },
};
