import { useSource } from "@swarmie/client-ui";
import { ProviderSettingsPanel } from "@swarmie/plugin-providers-browser/views";
import { Button } from "@swarmie/ui-web";
import { ArrowLeft } from "lucide-react";
import { useCallback } from "react";

import { useClient } from "../client/context";

export function ProviderSettings() {
  const { providers, shell, navigate } = useClient();
  const layout = useSource(shell);
  const accountId = layout.params.providerAccount;
  const focusHeading = useCallback((heading: HTMLHeadingElement | null) => {
    heading?.focus();
  }, []);
  return (
    <>
      {accountId === undefined ? null : (
        <Button
          size="sm"
          variant="ghost"
          className="mb-7"
          onPress={() => {
            navigate({ params: {} });
          }}
        >
          <ArrowLeft />
          Back to providers
        </Button>
      )}
      <h1
        key={accountId ?? "providers"}
        ref={focusHeading}
        tabIndex={-1}
        className="mb-6 text-2xl font-medium outline-none"
      >
        {accountId === undefined ? "Providers" : "Account configuration"}
      </h1>
      <ProviderSettingsPanel
        providers={providers}
        accountId={accountId}
        onConfigure={(id) => {
          navigate({ params: { providerAccount: id } });
        }}
      />
    </>
  );
}
