import { BrandMark, Button, Chip, ResponsiveDialog, Surface } from "@swarmie/ui-web";
import { useId, useState } from "react";
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
  const headingId = useId();
  const title = {
    provider: "Choose your provider",
    configuration: "Configure provider",
    ready: "Ready to work",
  }[step];
  return (
    <ResponsiveDialog isOpen>
      <ResponsiveDialog.Backdrop isDismissable={false} isKeyboardDismissDisabled>
        <ResponsiveDialog.Container size="lg" placement="center">
          <ResponsiveDialog.Dialog
            layout="guided"
            aria-labelledby={headingId}
            className="@container/onboarding overflow-hidden"
          >
            <div className="grid max-h-[min(656px,calc(100dvh-64px))] min-h-0 grid-cols-1 overflow-y-auto @min-[720px]/onboarding:h-[min(656px,calc(100dvh-64px))] @min-[720px]/onboarding:grid-cols-[minmax(240px,0.44fr)_minmax(0,1fr)] @min-[720px]/onboarding:overflow-hidden">
              <Surface
                render={(props) => <aside {...props} />}
                variant="secondary"
                className="min-h-0 rounded-none border-b border-border @min-[720px]/onboarding:border-r @min-[720px]/onboarding:border-b-0"
              >
                <div className="flex h-full min-h-0 flex-col gap-8 p-8 @min-[720px]/onboarding:overflow-y-auto">
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
                      Swarmie account <Chip size="sm">Optional</Chip>
                    </p>
                    <p className="text-xs leading-relaxed text-muted">
                      Use your providers without a Swarmie account.
                    </p>
                    <Button variant="outline" isDisabled>
                      Sign in
                    </Button>
                  </div>
                </div>
              </Surface>
              <Surface
                render={(props) => <section {...props} />}
                variant="tertiary"
                className="flex min-h-0 flex-col gap-8 rounded-none p-8 @min-[720px]/onboarding:overflow-y-auto"
              >
                <ResponsiveDialog.Header>
                  <h2 id={headingId} className="text-title font-medium">
                    {title}
                  </h2>
                </ResponsiveDialog.Header>
                <ResponsiveDialog.Body>
                  {step === "ready" ? (
                    <div className="grid gap-5">
                      <p>Your provider is configured.</p>
                      <Button onPress={onComplete}>Start a conversation</Button>
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
                </ResponsiveDialog.Body>
              </Surface>
            </div>
          </ResponsiveDialog.Dialog>
        </ResponsiveDialog.Container>
      </ResponsiveDialog.Backdrop>
    </ResponsiveDialog>
  );
}
