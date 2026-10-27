import { BrandMark, Button, Modal } from "@swarmie/ui-web";
import { useState } from "react";
import type { ReactNode } from "react";

import { GuideProgress } from "./guide-progress";
import type { GuideStep } from "./guide-progress";

/** Providers owns configuration; this guide only places it and tracks completion. */
export function OnboardingGuide({
  renderProviderConfiguration,
  onComplete,
}: {
  renderProviderConfiguration: (events: {
    onConfigured: () => void;
    onSelectionChange: (selected: boolean) => void;
  }) => ReactNode;
  onComplete: () => void;
}) {
  const [step, setStep] = useState<GuideStep>("provider");
  return (
    <Modal isOpen>
      <Modal.Backdrop isDismissable={false} isKeyboardDismissDisabled>
        <Modal.Container size="lg" placement="center">
          <Modal.Dialog layout="guided" aria-label="Get started with Swarmie">
            <div className="grid h-[min(656px,calc(100dvh-64px))] grid-cols-[minmax(240px,0.44fr)_minmax(0,1fr)]">
              <aside className="flex min-h-0 flex-col gap-8 overflow-y-auto border-r border-border bg-navigation-surface p-8">
                <div className="flex items-center gap-3 text-lg font-medium">
                  <BrandMark className="size-8" />
                  Swarmie
                </div>
                <div className="grid gap-2">
                  <h2 className="text-title font-medium">Get started</h2>
                  <p className="text-base text-muted">Configure your AI provider.</p>
                </div>
                <GuideProgress step={step} />
                <div className="mt-auto grid justify-items-start gap-3 border-t border-border pt-5">
                  <p className="text-sm">
                    Swarmie account{" "}
                    <span className="ml-2 rounded-md bg-default px-2 py-1 text-chrome text-muted">
                      Optional
                    </span>
                  </p>
                  <p className="text-xs leading-relaxed text-muted">
                    Use your providers without a Swarmie account.
                  </p>
                  <Button variant="outline" isDisabled>
                    Sign in
                  </Button>
                </div>
              </aside>
              <section className="flex min-h-0 flex-col gap-6 overflow-y-auto p-8">
                <Modal.Header>
                  <Modal.Heading scale="title">
                    {(() => {
                      if (step === "provider") {
                        return "Choose your provider";
                      }
                      if (step === "configuration") {
                        return "Configure provider";
                      }
                      return "Ready to work";
                    })()}
                  </Modal.Heading>
                </Modal.Header>
                <Modal.Body>
                  {step === "ready" ? (
                    <div className="grid gap-5">
                      <p>Your provider is configured.</p>
                      <Button tone="brand" shape="panel" size="lg" onPress={onComplete}>
                        Start a conversation
                      </Button>
                    </div>
                  ) : (
                    renderProviderConfiguration({
                      onConfigured: () => {
                        setStep("ready");
                      },
                      onSelectionChange: (selected) => {
                        setStep(selected ? "configuration" : "provider");
                      },
                    })
                  )}
                </Modal.Body>
              </section>
            </div>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}
