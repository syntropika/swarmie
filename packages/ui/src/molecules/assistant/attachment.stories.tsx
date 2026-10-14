import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, fireEvent, userEvent, waitFor, within } from "storybook/test";

import { FileAttachment, MessageAttachments, UserMessageAttachments } from "./attachment";
import { demoImage } from "./examples/fixtures";

function StagedAttachments() {
  const [removed, setRemoved] = useState<readonly string[]>([]);
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        {removed.includes("image") ? null : (
          <FileAttachment
            name="component-preview.png"
            detail="Image · Ready"
            image={{ src: demoImage, alt: "Illustrative shared document" }}
            previewLabel="Preview component image"
            closeLabel="Close image preview"
            removeLabel="Remove attachment"
            onRemove={() => {
              setRemoved((items) => [...items, "image"]);
            }}
          />
        )}
        {removed.includes("document") ? null : (
          <FileAttachment
            name="contract.pdf"
            detail="Uploading document"
            status="uploading"
            progress={42}
            removeLabel="Remove attachment"
            onRemove={() => {
              setRemoved((items) => [...items, "document"]);
            }}
          />
        )}
        {removed.includes("file") ? null : (
          <FileAttachment
            name="notes.txt"
            detail="Upload failed. Try adding this file again."
            status="failed"
            removeLabel="Remove attachment"
            onRemove={() => {
              setRemoved((items) => [...items, "file"]);
            }}
          />
        )}
      </div>
      {removed.length > 0 ? <output>Attachment removed</output> : null}
    </div>
  );
}

const meta = {
  title: "Assistant/Sources & attachments/Attachments",
  component: FileAttachment,
  args: { name: "contract.pdf", detail: "Document · Ready" },
  parameters: {
    layout: "centered",
    docs: {
      description: {
        component:
          "Assistant UI attachment structure adapted to Swarmie. Composer tiles preserve image previews, filename tooltips, upload/error overlays, removal, and their 200ms entrance. Sent images use the larger thumbnail. Received-file rows remain host-controlled. Uploads and preview URLs belong to the caller.",
      },
    },
  },
} satisfies Meta<typeof FileAttachment>;
export default meta;
type Story = StoryObj<typeof meta>;

export const ComposerTiles: Story = {
  render: () => <StagedAttachments />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const preview = canvas.getByRole("button", { name: "Preview component image" });
    await userEvent.click(preview);
    const dialog = within(document.body).getByRole("dialog", { name: "Preview component image" });
    await waitFor(async () => {
      await expect(dialog).toBeVisible();
    });
    // userEvent's synthetic key dispatch does not perform native dialog cancellation.
    // Exercise the native cancel event; browser verification separately sends a real Escape key.
    await fireEvent(dialog, new Event("cancel", { cancelable: true }));
    await waitFor(async () => {
      await expect(preview).toHaveFocus();
    });
    await userEvent.click(
      canvas.getByRole("button", { name: "Remove attachment: component-preview.png" }),
    );
    await expect(canvas.getByText("Attachment removed")).toBeVisible();
    await expect(
      canvas.queryByRole("button", { name: "Preview component image" }),
    ).not.toBeInTheDocument();
  },
};

export const MessageImage: Story = {
  render: () => (
    <UserMessageAttachments
      label="Message attachments"
      attachments={[
        {
          id: "image",
          name: "component-preview.png",
          detail: "Image · 4.2 KB",
          kind: "image",
          image: { src: demoImage, alt: "Illustrative shared document" },
        },
      ]}
      previewLabel={(name) => `Preview ${name}`}
      closeLabel="Close image preview"
    />
  ),
};

function ReceivedFilesExample() {
  const [opened, setOpened] = useState("");
  return (
    <div className="w-80 max-w-full space-y-3">
      <MessageAttachments
        attachments={[
          {
            id: "image",
            name: "component-preview.png",
            detail: "4.2 KB",
            kind: "image",
            image: { src: demoImage, alt: "Illustrative shared document" },
          },
          { id: "document", name: "contract.pdf", detail: "340 KB · 12 pages", kind: "document" },
          { id: "file", name: "session-notes.txt", detail: "2 KB", kind: "file" },
        ]}
        onOpen={setOpened}
      />
      <output className="text-ui">{opened ? `Open requested: ${opened}` : ""}</output>
    </div>
  );
}

export const ReceivedFiles: Story = { render: () => <ReceivedFilesExample /> };
