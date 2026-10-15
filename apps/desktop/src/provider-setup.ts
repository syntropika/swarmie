// Connection forms and native login flows stay transient; no credentials enter workspace snapshots.
export type AgentToolField = { id: string; label: string; secret?: boolean; placeholder?: string };
export type AgentToolMethod = {
  id: string;
  label: string;
  kind: "api_key" | "browser" | "device" | "existing";
  fields?: AgentToolField[];
  help?: string;
};
export type AgentToolProvider = { id: string; label: string; methods: AgentToolMethod[] };
export type AgentToolFlow = {
  id: string;
  status: "pending" | "connected" | "completed" | "failed" | "cancelled";
  operation?: "install" | "connect";
  message?: string;
  authorizationUrl?: string;
  userCode?: string;
  expiresAt?: string;
  inputFields?: AgentToolField[];
};
export type AgentToolSetup = {
  harness: string;
  installed: boolean;
  authState: "authenticated" | "unauthenticated" | "unknown";
  manual?: boolean;
  storage: { persistent: boolean; description: string };
  providers: AgentToolProvider[];
  message?: string;
  flow?: AgentToolFlow;
  connection?: { provider: string; method: string; storage: string; verified: boolean };
  installation?: { supported: boolean; message?: string };
};
export type AgentToolConnect = {
  harness: string;
  provider: string;
  method: string;
  values: Record<string, string>;
  storage: "secure" | "session";
};

export function preferredConnectionMethod(provider?: AgentToolProvider, setup?: AgentToolSetup) {
  return (
    provider?.methods.find((method) => method.id === setup?.connection?.method) ??
    (setup?.authState === "authenticated"
      ? provider?.methods.find((method) => method.kind === "existing")
      : undefined) ??
    provider?.methods.find((method) => method.kind === "browser" || method.kind === "device") ??
    provider?.methods.find((method) => method.kind === "api_key") ??
    provider?.methods[0]
  );
}

export function setupAuthorizationUrl(url?: string): string | undefined {
  try {
    if (url === undefined || url.length === 0) {
      return undefined;
    }
    const parsed = new URL(url);
    return parsed.protocol === "https:" && !parsed.username && !parsed.password
      ? parsed.href
      : undefined;
  } catch {
    return undefined;
  }
}
