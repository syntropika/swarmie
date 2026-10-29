import type { Snapshot, UiStore } from "@swarmie/client-ui";
import type { OnboardingState } from "@swarmie/plugin-onboarding-browser";
import type { ProvidersState } from "@swarmie/plugin-providers-browser";

/** Only a loaded node catalog can establish whether first-use configuration is needed. */
export function observeOnboarding(
  onboarding: UiStore<Readonly<OnboardingState>>,
  providers: Snapshot<Readonly<ProvidersState>>,
) {
  function refresh() {
    const guide = onboarding.getSnapshot();
    const catalog = providers.getSnapshot();
    if (guide.completed || guide.open || !catalog.ready || catalog.error !== null) {
      return;
    }
    onboarding.update((current) => {
      const latest = providers.getSnapshot();
      if (current.completed || current.open || !latest.ready || latest.error !== null) {
        return current;
      }
      const configured = latest.accounts.length > 0;
      return { completed: configured, open: !configured };
    });
  }
  const unsubscribe = providers.subscribe(refresh);
  refresh();
  return unsubscribe;
}
