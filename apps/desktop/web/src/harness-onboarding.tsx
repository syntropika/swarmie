import { X } from "lucide-react";

import { CodingAgentsPanel } from "~/web/src/coding-agents-panel";
import { Button } from "~/web/src/components/ui/button";
import { Dialog } from "~/web/src/components/ui/dialog";
import type { WorkspaceStore } from "~/web/src/workspace";

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
    <Dialog label="Agent tool setup" onClose={onFinish} variant="onboarding">
      <header className="onboarding-header">
        <span className="onboarding-wordmark">swarmie</span>
        <Button
          variant="ghost"
          size="icon"
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
          variant="link"
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
    </Dialog>
  );
}
