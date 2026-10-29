import { ProviderConfigurationForm } from "./configuration-form";
import type {
  ProviderAccount,
  ProviderChoice,
  ProviderConfigurationMethod,
  ProvidersApi,
} from "./index";
import { OAuthFlow } from "./oauth-flow";

/** Resolve a capability once before mounting the component that requires it. */
export function ProviderMethodView({
  providers,
  provider,
  method,
  onConfigured,
  onBusyChange,
  accountId,
}: {
  providers: ProvidersApi;
  provider: ProviderChoice;
  method: ProviderConfigurationMethod;
  onConfigured: (account: ProviderAccount) => void;
  onBusyChange: (busy: boolean) => void;
  accountId?: string | undefined;
}) {
  if (method.kind === "oauth") {
    const { authorization } = providers;
    return authorization === null ? (
      <p className="text-sm text-muted">Account authorization is unavailable on this node.</p>
    ) : (
      <OAuthFlow
        provider={provider}
        method={method.id}
        api={authorization}
        onConnected={onConfigured}
        accountId={accountId}
      />
    );
  }
  const { configuration } = providers;
  if (configuration === null) {
    return (
      <p className="text-sm text-muted">Provider configuration is unavailable on this node.</p>
    );
  }
  return (
    <ProviderConfigurationForm
      api={configuration}
      discovery={providers.discovery}
      providerId={provider.id}
      method={method}
      onConfigured={onConfigured}
      onBusyChange={onBusyChange}
    />
  );
}
