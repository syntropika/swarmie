import {
  ActionMenuPopover,
  Button,
  ChromeButton,
  Dropdown,
  Icon,
  Label,
  Modal,
  Separator,
} from "@swarmie/ui-web";
import { useState } from "react";

import { useClient } from "../client/context";
import { copyText, useChat } from "../controls";

export function ConversationMenu() {
  const { archive, togglePin, renameConversation, createConversation } = useClient();
  const chat = useChat();
  const [renaming, setRenaming] = useState(false);
  const [title, setTitle] = useState("");
  const markdown = () =>
    chat.messages
      .getSnapshot()
      .map((message) => message.text)
      .join("\n\n");
  const link = () => {
    const url = new URL(window.location.href);
    url.searchParams.set("conversation", chat.id);
    return url.href;
  };
  return (
    <>
      <Dropdown>
        <ChromeButton size="application" aria-label="Conversation menu">
          <Icon name="dots" />
        </ChromeButton>
        <ActionMenuPopover>
          <Dropdown.Menu
            aria-label="Conversation options"
            onAction={(key) => {
              if (key === "rename") {
                setTitle(chat.title);
                setRenaming(true);
              }
              if (key === "pin") {
                togglePin(chat);
              }
              if (key === "side") {
                createConversation();
              }
              if (key === "archive") {
                archive(chat);
              }
              if (key === "window") {
                window.open(link(), "_blank", "noopener,noreferrer");
              }
              if (key === "share") {
                copyText(link());
              }
            }}
          >
            <Dropdown.Item id="rename" textValue="Rename">
              <Icon name="rename" />
              <Label>Rename</Label>
            </Dropdown.Item>
            <Dropdown.Item id="pin" textValue={chat.pinned ? "Unpin" : "Pin"}>
              <Icon name="pin" />
              <Label>{chat.pinned ? "Unpin" : "Pin"}</Label>
            </Dropdown.Item>
            <Separator />
            <Dropdown.Item id="side" textValue="New side chat">
              <Icon name="chat" />
              <Label>New side chat</Label>
            </Dropdown.Item>
            <Dropdown.SubmenuTrigger>
              <Dropdown.Item id="fork" textValue="Fork">
                <Icon name="fork" />
                <Label>Fork</Label>
                <Dropdown.SubmenuIndicator />
              </Dropdown.Item>
              <ActionMenuPopover>
                <Dropdown.Menu aria-label="Fork conversation">
                  <Dropdown.Item id="fork-current" textValue="Fork conversation" isDisabled>
                    <Label>Fork conversation</Label>
                  </Dropdown.Item>
                  <Dropdown.Item
                    id="fork-message"
                    textValue="Fork from selected message"
                    isDisabled
                  >
                    <Label>Fork from selected message</Label>
                  </Dropdown.Item>
                </Dropdown.Menu>
              </ActionMenuPopover>
            </Dropdown.SubmenuTrigger>
            <Separator />
            <Dropdown.Item id="share" textValue="Share">
              <Icon name="share" />
              <Label>Share</Label>
            </Dropdown.Item>
            <Dropdown.SubmenuTrigger>
              <Dropdown.Item id="copy" textValue="Copy">
                <Icon name="copy" />
                <Label>Copy</Label>
                <Dropdown.SubmenuIndicator />
              </Dropdown.Item>
              <ActionMenuPopover>
                <Dropdown.Menu
                  aria-label="Copy conversation"
                  onAction={(key) => {
                    if (key === "deeplink") {
                      copyText(link());
                    }
                    if (key === "markdown") {
                      copyText(markdown());
                    }
                  }}
                >
                  <Dropdown.Item id="deeplink" textValue="Copy deeplink">
                    <Icon name="copy" />
                    <Label>Copy deeplink</Label>
                  </Dropdown.Item>
                  <Dropdown.Item id="markdown" textValue="Copy as Markdown">
                    <Icon name="copy" />
                    <Label>Copy as Markdown</Label>
                  </Dropdown.Item>
                  <Dropdown.Item id="directory" textValue="Copy working directory" isDisabled>
                    <Icon name="project" />
                    <Label>Copy working directory</Label>
                  </Dropdown.Item>
                </Dropdown.Menu>
              </ActionMenuPopover>
            </Dropdown.SubmenuTrigger>
            <Separator />
            <Dropdown.Item id="window" textValue="Open in new window">
              <Icon name="external" />
              <Label>Open in new window</Label>
            </Dropdown.Item>
            <Separator />
            <Dropdown.Item id="archive" textValue="Archive">
              <Icon name="archive" />
              <Label>Archive</Label>
            </Dropdown.Item>
          </Dropdown.Menu>
        </ActionMenuPopover>
      </Dropdown>
      <Modal isOpen={renaming} onOpenChange={setRenaming}>
        <Modal.Backdrop>
          <Modal.Container>
            <Modal.Dialog aria-label="Rename conversation">
              <Modal.Header>
                <Modal.Heading>Rename conversation</Modal.Heading>
              </Modal.Header>
              <form
                onSubmit={(event) => {
                  event.preventDefault();
                  if (title.trim() !== "") {
                    renameConversation(chat, title.trim());
                    setRenaming(false);
                  }
                }}
              >
                <label className="grid gap-2 text-sm">
                  Title
                  <input
                    autoFocus
                    value={title}
                    onChange={(event) => {
                      setTitle(event.target.value);
                    }}
                    className="rounded-lg border border-border bg-default px-3 py-2"
                  />
                </label>
                <Modal.Footer>
                  <Button
                    variant="tertiary"
                    onPress={() => {
                      setRenaming(false);
                    }}
                  >
                    Cancel
                  </Button>
                  <Button type="submit" isDisabled={title.trim() === ""}>
                    Save
                  </Button>
                </Modal.Footer>
              </form>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal>
    </>
  );
}
