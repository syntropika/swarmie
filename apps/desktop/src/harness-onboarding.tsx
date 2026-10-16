import { Button, Modal } from "@swarmie/ui-web";
import { X } from "lucide-react";

import { CodingAgentsPanel } from "~/src/coding-agents-panel";
import type { WorkspaceStore } from "~/src/workspace";

export function HarnessOnboarding({
  store,
  nodeName,
  onFinish,
}: {
  store: WorkspaceStore;
  nodeName: string;
  onFinish: () => void;
}) {
  return (
    <Modal
      isOpen
      onOpenChange={(next) => {
        if (!next) {
          onFinish();
        }
      }}
    >
      <Modal.Backdrop isDismissable>
        <Modal.Container>
          <Modal.Dialog aria-label="Agent tool setup" data-workspace-part="harness-onboarding">
            <header className="onboarding-header">
              <span className="onboarding-wordmark">swarmie</span>
              <Button
                variant="tertiary"
                size="md"
                isIconOnly
                aria-label="Skip agent tool setup"
                onClick={() => {
                  onFinish();
                }}
              >
                <X size={18} />
              </Button>
            </header>
            <h2 className="onboarding-title">Set up your agent tools</h2>
            <p className="onboarding-description">
              Choose the AI tools you want to use in Swarmie, such as Codex or Claude Code.
            </p>
            <CodingAgentsPanel store={store} nodeName={nodeName} />
            <footer className="onboarding-footer">
              <Button
                variant="tertiary"
                onClick={() => {
                  onFinish();
                }}
              >
                Skip setup
              </Button>
              <Button
                onClick={() => {
                  onFinish();
                }}
              >
                Finish setup
              </Button>
            </footer>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}
