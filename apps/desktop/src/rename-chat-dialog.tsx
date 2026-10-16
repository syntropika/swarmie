import { Button, Input, Label, Modal, TextField } from "@swarmie/ui-web";
import { useMutation } from "@tanstack/react-query";
import { useState } from "react";

import { errorMessage } from "~/src/action-boundary";
import type { Thread } from "~/src/protocol";
import type { WorkspaceStore } from "~/src/workspace";

export function RenameChatDialog({
  thread,
  store,
  onClose,
}: {
  thread: Pick<Thread, "id" | "title">;
  store: WorkspaceStore;
  onClose: () => void;
}) {
  const [title, setTitle] = useState(thread.title);
  const rename = useMutation({
    mutationFn: async () => {
      await store.rename(thread.id, title.trim());
    },
    onSuccess: onClose,
    retry: false,
  });
  const canSave = title.trim().length > 0 && !rename.isPending;
  return (
    <Modal.Backdrop
      isOpen
      isDismissable={!rename.isPending}
      isKeyboardDismissDisabled={rename.isPending}
      onOpenChange={(open) => {
        if (!open && !rename.isPending) {
          onClose();
        }
      }}
    >
      <Modal.Container size="sm">
        <Modal.Dialog aria-label="Rename chat">
          <form
            onSubmit={(event) => {
              event.preventDefault();
              if (canSave) {
                rename.mutate();
              }
            }}
          >
            <Modal.Header>
              <Modal.Heading>Rename chat</Modal.Heading>
            </Modal.Header>
            <Modal.Body>
              <TextField value={title} onChange={setTitle} isDisabled={rename.isPending}>
                <Label>Chat name</Label>
                <Input
                  autoFocus
                  onFocus={(event) => {
                    event.target.select();
                  }}
                />
              </TextField>
              {rename.isError && (
                <p className="mt-4 text-danger" role="alert">
                  {errorMessage(rename.error)}
                </p>
              )}
            </Modal.Body>
            <Modal.Footer>
              <Button variant="tertiary" size="md" isDisabled={rename.isPending} onClick={onClose}>
                Cancel
              </Button>
              <Button
                type="submit"
                variant="primary"
                size="md"
                isDisabled={!canSave}
                isPending={rename.isPending}
              >
                Save
              </Button>
            </Modal.Footer>
          </form>
        </Modal.Dialog>
      </Modal.Container>
    </Modal.Backdrop>
  );
}
