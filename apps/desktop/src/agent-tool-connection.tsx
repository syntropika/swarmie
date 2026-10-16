import { Button, Input, Select, ListBox } from "@swarmie/ui-web";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Copy, ExternalLink, LoaderCircle, X } from "lucide-react";
import {
  isValidElement,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type { ReactNode, SubmitEvent } from "react";

import { startAction, errorReporter } from "~/src/action-boundary";
import { nodeKey } from "~/src/node-scope";
import { copyText } from "~/src/protocol";
import { preferredConnectionMethod, setupAuthorizationUrl } from "~/src/provider-setup";
import type { AgentToolFlow, AgentToolSetup } from "~/src/provider-setup";
import { hasText, hasNumber, nonEmptyText } from "~/src/value-presence";
import type { WorkspaceStore } from "~/src/workspace";

type ConnectionProps = {
  store: WorkspaceStore;
  harness: string;
  name: string;
  nodeName: string;
  onClose: () => void;
  fallback: ReactNode;
};
export function AgentToolConnection(props: ConnectionProps) {
  const { store, harness, name, nodeName, onClose } = props;
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot);
  const scope = state.node ? nodeKey(state.node) : "";
  const query = useQuery({
    queryKey: ["agent-tool-setup", scope, harness],
    queryFn: async () => await store.agentToolsSetup(harness),
    enabled: state.connected && scope.length > 0,
  });
  if (query.data !== undefined) {
    return (
      <ConnectionForm
        key={`${scope}:${harness}:${state.connected}`}
        {...props}
        initialSetup={query.data}
      />
    );
  }
  return (
    <section className="agent-tool-connection" aria-label={`Connect ${name}`}>
      <div className="agent-tool-connection-heading">
        <h3>Connect {name}</h3>
        <Button
          variant="tertiary"
          size="sm"
          isIconOnly
          aria-label={`Close ${name} connection setup`}
          onClick={onClose}
        >
          <X size={14} />
        </Button>
      </div>
      <ConnectionLoading
        connected={state.connected}
        failed={query.isError}
        nodeName={nodeName}
        retry={() => {
          startAction(query.refetch(), store.setError.bind(store));
        }}
      />
    </section>
  );
}
function useConnectionForm({
  store,
  harness,
  initialSetup,
}: ConnectionProps & { initialSetup: AgentToolSetup }) {
  const client = useQueryClient();
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot);
  const scope = state.node ? nodeKey(state.node) : "";
  const [setup, setSetup] = useState(initialSetup);
  const initialProvider = selectedProvider(initialSetup);
  const [providerId, setProviderId] = useState(initialProvider?.id ?? "");
  const [methodId, setMethodId] = useState(
    preferredConnectionMethod(initialProvider, initialSetup)?.id ?? "",
  );
  const [storage, setStorage] = useState<"secure" | "session">(
    initialSetup.storage.persistent ? "secure" : "session",
  );
  const [flow, setFlow] = useState(initialSetup.flow);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [copied, setCopied] = useState(false);
  const [pollCycle, setPollCycle] = useState(0);
  const form = useRef<HTMLFormElement>(null);
  const flowRef = useRef<AgentToolFlow | undefined>(initialSetup.flow);
  const generation = useRef(0);
  const id = useId();
  const clearInputs = useCallback(() => {
    // Clear credential inputs without dispatching a form reset: Radix selects
    // own their nonsecret provider/method choices and listen for reset events.
    for (const input of form.current?.querySelectorAll<HTMLInputElement>("input[name]") ?? []) {
      input.value = "";
    }
  }, []);
  const current = useCallback(() => {
    const latest = store.getSnapshot();
    return latest.connected && scope !== "" && !!latest.node && nodeKey(latest.node) === scope;
  }, [scope, store]);
  const applyFlow = useCallback((next: AgentToolFlow) => {
    if (
      flowRef.current?.id === next.id &&
      flowRef.current.status !== "pending" &&
      next.status === "pending"
    ) {
      return;
    }
    flowRef.current = next;
    setFlow(next);
    if (next.status === "connected") {
      setNotice(nonEmptyText(next.message) ?? "Connection saved. Check its status below.");
    }
    if (next.status === "completed") {
      setNotice(
        nonEmptyText(next.message) ?? "Installation complete. Connect your account to get started.",
      );
    }
    if (next.status === "cancelled") {
      setNotice(next.operation === "install" ? "Installation cancelled." : "Sign-in cancelled.");
    }
    if (next.status === "failed") {
      setNotice("");
      setError(
        nonEmptyText(next.message) ??
          "Sign-in did not complete. Try again or use another connection method.",
      );
    }
  }, []);
  const refresh = useCallback(async () => {
    const next = await store.agentToolsSetup(harness);
    if (!Array.isArray(next.providers)) {
      throw new TypeError("Connection options are unavailable");
    }
    client.setQueryData(["agent-tool-setup", scope, harness], next);
    return next;
  }, [client, scope, harness, store]);
  const selectInstalledConnection = useCallback(
    (next: AgentToolSetup) => {
      const provider =
        next.providers.find((item) => item.id === next.connection?.provider) ??
        next.providers.at(0);
      setProviderId(provider?.id ?? "");
      setMethodId(preferredConnectionMethod(provider, next)?.id ?? "");
    },
    [setProviderId, setMethodId],
  );

  useEffect(() => {
    generation.current += 1;
    return () => {
      generation.current += 1;
      const version = generation.current;
      clearInputs();
      const pending = flowRef.current;
      // React's development remount must not cancel an adopted provider sign-in.
      queueMicrotask(() => {
        if (generation.current !== version || pending?.status !== "pending" || !current()) {
          return;
        }
        startAction(store.agentToolsCancel(pending.id), () => {
          store.setError(new Error("Pending sign-in cancellation could not be confirmed."));
        });
      });
    };
  }, [clearInputs, current, store]);

  const flowId = flow?.id;
  const flowStatus = flow?.status;
  useEffect(() => {
    if (flowStatus !== "pending" || flowId === undefined || !state.connected) {
      return undefined;
    }
    const version = generation.current;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await store.agentToolsFlow(flowId);
        if (disposed || version !== generation.current || !current()) {
          return;
        }
        applyFlow(next);
        if (next.status === "connected" || next.status === "completed") {
          const updated = await refresh();
          // Terminal progress disposes this polling loop; its same-node refresh
          // still belongs to the mounted form unless the setup generation changed.
          if (version !== generation.current || !current()) {
            return;
          }
          setSetup(updated);
          if (next.operation === "install") {
            selectInstalledConnection(updated);
          }
        }
        if (next.status === "pending") {
          timer = setTimeout(() => {
            startAction(poll(), store.setError.bind(store));
          }, 1500);
        }
      } catch {
        if (!disposed && version === generation.current) {
          setError(
            "Sign-in progress could not be checked. Reconnect to this computer and check again.",
          );
        }
      }
    };
    timer = setTimeout(
      () => {
        startAction(poll(), store.setError.bind(store));
      },
      pollCycle === 0 ? 1000 : 0,
    );
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [
    applyFlow,
    current,
    flowId,
    flowStatus,
    pollCycle,
    refresh,
    selectInstalledConnection,
    state.connected,
    store,
  ]);

  const provider = selectedProvider(setup, providerId);
  const method = selectedMethod(setup, provider, methodId);
  const pending = flow?.status === "pending";
  const needsSignInSupport = needsManagedSignIn(harness, setup);
  const storesCredential =
    method?.kind === "api_key" || (harness === "cursor" && method?.kind === "browser");
  const fields = pending ? flow?.inputFields : method?.fields;
  const authorizationUrl = setupAuthorizationUrl(flow?.authorizationUrl);
  const chooseProvider = (nextId: string) => {
    clearInputs();
    setFlow(undefined);
    flowRef.current = undefined;
    setError("");
    setNotice("");
    setProviderId(nextId);
    const next = setup.providers.find((item) => item.id === nextId);
    setMethodId(preferredConnectionMethod(next)?.id ?? "");
  };

  const cancelReturnedFlow = (next: AgentToolFlow) => {
    if (next.status !== "pending" || !current()) {
      return;
    }
    startAction(store.agentToolsCancel(next.id), () => {
      store.setError(new Error("Pending sign-in cancellation could not be confirmed."));
    });
  };
  const submit = async (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!current() || busy || !provider || !method) {
      return;
    }
    const version = generation.current;
    const values: Record<string, string> = {};
    for (const [field, value] of credentialValues(event.currentTarget)) {
      values[field] = value;
    }
    // The only owner of submitted values after this point is the private RPC.
    clearInputs();
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const next =
        flow?.status === "pending"
          ? await store.agentToolsInput(flow.id, values)
          : await store.agentToolsConnect({
              harness,
              provider: provider.id,
              method: method.id,
              values,
              storage,
            });
      if (version !== generation.current || !current()) {
        cancelReturnedFlow(next);
        return;
      }
      applyFlow(next);
      if (next.status === "connected") {
        const updated = await refresh();
        if (version === generation.current && current()) {
          setSetup(updated);
          if (next.operation === "install") {
            selectInstalledConnection(updated);
          }
        }
      }
    } catch {
      if (version === generation.current) {
        setError("Could not connect. Check your sign-in details and try again.");
      }
    } finally {
      for (const key of Object.keys(values)) {
        values[key] = "";
      }
      if (version === generation.current) {
        setBusy(false);
      }
    }
  };
  const cancel = async () => {
    if (!flow || busy || !current()) {
      return;
    }
    const version = generation.current;
    setBusy(true);
    clearInputs();
    setError("");
    try {
      const next = await store.agentToolsCancel(flow.id);
      if (version !== generation.current || !current()) {
        return;
      }
      applyFlow(next);
      if (next.status === "connected" || next.status === "completed") {
        const updated = await refresh();
        if (version === generation.current && current()) {
          setSetup(updated);
          if (next.operation === "install") {
            selectInstalledConnection(updated);
          }
        }
      } else if (next.status === "pending") {
        setNotice(
          flow?.operation === "install" ? "Cancelling installation…" : "Cancelling sign-in…",
        );
      } else if (next.status === "cancelled") {
        setNotice(flow?.operation === "install" ? "Installation cancelled." : "Sign-in cancelled.");
      }
    } catch {
      if (version === generation.current) {
        setError("Sign-in could not be cancelled. Check its status before starting again.");
      }
    } finally {
      if (version === generation.current) {
        setBusy(false);
      }
    }
  };
  const install = async () => {
    if (!current() || busy || pending) {
      return;
    }
    const version = generation.current;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const next = await store.agentToolsInstall(harness);
      if (version !== generation.current || !current()) {
        cancelReturnedFlow(next);
        return;
      }
      applyFlow(next);
      if (next.status === "completed") {
        const updated = await refresh();
        if (version === generation.current && current()) {
          setSetup(updated);
          selectInstalledConnection(updated);
        }
      }
    } catch {
      if (version === generation.current) {
        setError("Installation could not start. Try again or use the installation guide.");
      }
    } finally {
      if (version === generation.current) {
        setBusy(false);
      }
    }
  };
  const retrySetup = async () => {
    if (!current()) {
      return;
    }
    const version = generation.current;
    setLoading(true);
    setError("");
    try {
      const next = await refresh();
      if (version !== generation.current || !current()) {
        return;
      }
      setSetup(next);
      setStorage(next.storage.persistent ? "secure" : "session");
      if (next.flow) {
        applyFlow(next.flow);
      }
    } catch {
      if (version === generation.current) {
        setError("Connection options could not be loaded. Reconnect and try again.");
      }
    } finally {
      if (version === generation.current) {
        setLoading(false);
      }
    }
  };
  const disconnect = async () => {
    if (!current() || busy) {
      return;
    }
    const version = generation.current;
    setBusy(true);
    clearInputs();
    setError("");
    try {
      const next = await store.agentToolsDisconnect(harness);
      if (version !== generation.current || !current()) {
        return;
      }
      setSetup(next);
      setFlow(undefined);
      flowRef.current = undefined;
      setNotice("Connection removed from Swarmie. Your agent tool's own login is unchanged.");
    } catch {
      if (version === generation.current) {
        setError("The connection could not be removed. Reconnect and try again.");
      }
    } finally {
      if (version === generation.current) {
        setBusy(false);
      }
    }
  };

  return {
    state,
    scope,
    setup,
    provider,
    method,
    pending,
    flow,
    form,
    id,
    storage,
    loading,
    busy,
    error,
    notice,
    copied,
    fields,
    authorizationUrl,
    needsSignInSupport,
    storesCredential,
    chooseProvider,
    submit,
    cancel,
    install,
    retrySetup,
    disconnect,
    clearInputs,
    setMethodId,
    setFlow,
    flowRef,
    setError,
    setNotice,
    setPollCycle,
    setStorage,
    setCopied,
  };
}
type ConnectionController = ReturnType<typeof useConnectionForm>;
function ConnectionForm(props: ConnectionProps & { initialSetup: AgentToolSetup }) {
  const controller = useConnectionForm(props);
  return <ConnectionView {...props} controller={controller} />;
}
function ConnectionView({
  controller,
  name,
  nodeName,
  onClose,
  harness,
  fallback,
}: ConnectionProps & { controller: ConnectionController }) {
  const { state, setup, pending, flow, loading, busy, error, notice, disconnect, setError } =
    controller;
  const installing =
    (!setup.installed && setup.manual !== true) || (pending && flow?.operation === "install");
  return (
    <section className="agent-tool-connection" aria-label={`Connect ${name}`}>
      <div className="agent-tool-connection-heading">
        <h3>
          {installing ? "Install" : "Connect"} {name}
        </h3>
        <Button
          variant="tertiary"
          size="sm"
          isIconOnly
          aria-label={`Close ${name} connection setup`}
          onClick={onClose}
        >
          <X size={14} />
        </Button>
      </div>
      {!loading && state.connected && setup.connection !== undefined && (
        <div className="agent-tool-current-connection">
          <p>
            <Check size={14} />
            {setup.connection.verified ? "Verified connection" : "Connection configured"}
          </p>
          <p>{storageLabel(setup.connection.storage, nodeName)}</p>
          <Button
            variant="tertiary"
            size="sm"
            disabled={busy || pending}
            onClick={() => {
              startAction(disconnect(), errorReporter(setError));
            }}
          >
            Disconnect in Swarmie
          </Button>
        </div>
      )}
      {!loading && state.connected && hasText(setup.message) && <p>{setup.message}</p>}
      <ConnectionBody
        controller={controller}
        name={name}
        nodeName={nodeName}
        harness={harness}
        fallback={fallback}
      />
      {hasText(notice) && <output className="agent-tool-connection-notice">{notice}</output>}
      {hasText(error) && (
        <p role="alert" className="agent-tool-connection-error">
          {error}
        </p>
      )}
    </section>
  );
}
function storageLabel(storage: string, nodeName: string) {
  if (storage === "secure") {
    return `Saved securely on ${nodeName}.`;
  }
  if (storage === "session") {
    return "Available for this runtime session only.";
  }
  return "Sign-in is managed by this agent tool's own account storage.";
}
function ConnectionBody(
  props: Pick<ConnectionProps, "name" | "nodeName" | "harness" | "fallback"> & {
    controller: ConnectionController;
  },
) {
  const { controller, nodeName } = props;
  const { loading, state, provider, method, setup, pending, flow } = controller;
  if (loading) {
    return (
      <output>
        <LoaderCircle size={14} className="animate-spin motion-reduce:animate-none" />
        Loading connection options…
      </output>
    );
  }
  if (!state.connected) {
    return <output>Reconnect to {nodeName} to manage this connection.</output>;
  }
  if (
    provider !== undefined &&
    method !== undefined &&
    (setup.installed || pending) &&
    !(pending && flow?.operation === "install")
  ) {
    return <ConnectionEditor {...props} provider={provider} method={method} />;
  }
  return <ConnectionInstall {...props} />;
}
function ConnectionEditor({
  controller,
  provider,
  method,
  name,
  nodeName,
  harness,
}: Pick<ConnectionProps, "name" | "nodeName" | "harness"> & {
  controller: ConnectionController;
  provider: NonNullable<ConnectionController["provider"]>;
  method: NonNullable<ConnectionController["method"]>;
}) {
  const {
    setup,
    form,
    id,
    pending,
    busy,
    error,
    fields,
    needsSignInSupport,
    chooseProvider,
    submit,
    cancel,
    install,
    clearInputs,
    setMethodId,
    setFlow,
    flowRef,
    setError,
    setNotice,
    setPollCycle,
  } = controller;
  return (
    <>
      <p>This connection is used for new chats on {nodeName}.</p>
      <form
        ref={form}
        onSubmit={(event) => {
          startAction(submit(event), errorReporter(setError));
        }}
        autoComplete="off"
      >
        {(harness === "pi" || harness === "opencode") && setup.providers.length > 1 && (
          <label htmlFor={`${id}-provider`} className="agent-tool-field">
            Model provider
            <Select
              aria-label={`${name} model provider`}
              value={provider.id}
              onChange={(key) => {
                if (typeof key === "string") {
                  chooseProvider(key);
                }
              }}
              isDisabled={busy || pending}
              placeholder="Select…"
            >
              <Select.Trigger id={`${id}-provider`}>
                <Select.Value />
                <Select.Indicator />
              </Select.Trigger>
              <Select.Popover>
                <ListBox aria-label={`${name} model provider`}>
                  {setup.providers
                    .map((item) => ({
                      id: item.id,
                      label: item.label,
                    }))
                    .map((option) => (
                      <ListBox.Item
                        key={option.id}
                        id={option.id}
                        textValue={option.label}
                        isDisabled={"disabled" in option && option.disabled === true}
                      >
                        {"icon" in option && isValidElement(option.icon) ? option.icon : null}
                        {option.label}
                        <ListBox.ItemIndicator />
                      </ListBox.Item>
                    ))}
                </ListBox>
              </Select.Popover>
            </Select>
          </label>
        )}
        {provider.methods.length > 1 && (
          <label htmlFor={`${id}-method`} className="agent-tool-field">
            Connection method
            <Select
              aria-label={`${name} connection method`}
              value={method.id}
              onChange={(key) => {
                if (typeof key === "string") {
                  ((value) => {
                    clearInputs();
                    setMethodId(value);
                    setFlow(undefined);
                    flowRef.current = undefined;
                    setError("");
                    setNotice("");
                  })(key);
                }
              }}
              isDisabled={busy || pending}
              placeholder="Select…"
            >
              <Select.Trigger id={`${id}-method`}>
                <Select.Value />
                <Select.Indicator />
              </Select.Trigger>
              <Select.Popover>
                <ListBox aria-label={`${name} connection method`}>
                  {provider.methods
                    .map((item) => ({
                      id: item.id,
                      label: item.label,
                    }))
                    .map((option) => (
                      <ListBox.Item
                        key={option.id}
                        id={option.id}
                        textValue={option.label}
                        isDisabled={"disabled" in option && option.disabled === true}
                      >
                        {"icon" in option && isValidElement(option.icon) ? option.icon : null}
                        {option.label}
                        <ListBox.ItemIndicator />
                      </ListBox.Item>
                    ))}
                </ListBox>
              </Select.Popover>
            </Select>
          </label>
        )}
        {hasText(method.help) && !pending && <p>{method.help}</p>}
        <LoginProgress controller={controller} name={name} />
        {fields?.map((field) => (
          <label
            className="agent-tool-field"
            key={`${method.id}:${field.id}`}
            htmlFor={`${id}-${field.id}`}
          >
            {field.label}
            <Input
              id={`${id}-${field.id}`}
              name={field.id}
              type={field.secret === false ? "text" : "password"}
              autoComplete="off"
              spellCheck={false}
              placeholder={field.placeholder}
              required
              disabled={busy}
            />
          </label>
        ))}
        <CredentialStorage
          controller={controller}
          method={method}
          name={name}
          harness={harness}
          nodeName={nodeName}
        />
        <div className="agent-tool-connection-actions">
          {(!pending || hasNumber(fields?.length)) && (
            <Button type="submit" size="sm" disabled={busy}>
              {busy && (
                <LoaderCircle size={13} className="animate-spin motion-reduce:animate-none" />
              )}
              {connectionActionLabel(pending, method.kind)}
            </Button>
          )}
          {pending && (
            <Button
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={() => {
                startAction(cancel(), errorReporter(setError));
              }}
            >
              Cancel sign-in
            </Button>
          )}
          {pending && error && (
            <Button
              variant="secondary"
              size="sm"
              disabled={busy}
              onClick={() => {
                setError("");
                setPollCycle((value) => value + 1);
              }}
            >
              Check sign-in status
            </Button>
          )}
        </div>
      </form>
      {needsSignInSupport && !pending && (
        <div className="agent-tool-install-guide">
          <p>
            Install managed sign-in support to connect in your browser.{" "}
            {setup.installation?.message}
          </p>
          <Button
            variant="outline"
            size="sm"
            disabled={busy}
            onClick={() => {
              startAction(install(), errorReporter(setError));
            }}
          >
            Install sign-in support
          </Button>
        </div>
      )}
    </>
  );
}
function ConnectionInstall({
  controller,
  name,
  nodeName,
  fallback,
}: Pick<ConnectionProps, "name" | "nodeName" | "fallback"> & { controller: ConnectionController }) {
  const { setup, pending, flow, busy, cancel, install, setError } = controller;
  return (
    <>
      {setup?.installation?.supported === true && (
        <>
          <p>
            {nonEmptyText(setup.installation.message) ??
              `Install ${name} on ${nodeName} to connect your account.`}
          </p>
          {pending ? (
            <>
              <output className="agent-tool-login-progress">
                <LoaderCircle size={14} className="animate-spin motion-reduce:animate-none" />
                <span>{nonEmptyText(flow?.message) ?? "Installing this agent tool…"}</span>
              </output>
              <Button
                variant="outline"
                size="sm"
                disabled={busy}
                onClick={() => {
                  startAction(cancel(), errorReporter(setError));
                }}
              >
                Cancel installation
              </Button>
            </>
          ) : (
            <Button
              size="sm"
              disabled={busy}
              onClick={() => {
                startAction(install(), errorReporter(setError));
              }}
            >
              {busy && (
                <LoaderCircle size={13} className="animate-spin motion-reduce:animate-none" />
              )}
              Install {name}
            </Button>
          )}
        </>
      )}
      <div className="agent-tool-install-guide">{fallback}</div>
    </>
  );
}

function needsManagedSignIn(harness: string, setup: AgentToolSetup) {
  return (
    ["cursor", "pi"].includes(harness) &&
    setup.installation?.supported === true &&
    !setup.providers.some((candidate) =>
      candidate.methods.some((choice) => choice.kind === "browser" || choice.kind === "device"),
    )
  );
}
function credentialValues(form: HTMLFormElement) {
  const fields: [string, string][] = [];
  for (const [field, value] of new FormData(form)) {
    if (typeof value !== "string") {
      throw new TypeError("Credential fields must contain text");
    }
    fields.push([field, value]);
  }
  return fields;
}

function ConnectionLoading({
  connected,
  failed,
  nodeName,
  retry,
}: {
  connected: boolean;
  failed: boolean;
  nodeName: string;
  retry: () => void;
}) {
  if (!connected) {
    return <output>Reconnect to {nodeName} to manage this connection.</output>;
  }
  if (failed) {
    return (
      <>
        <p role="alert" className="agent-tool-connection-error">
          Connection options could not be loaded. Reconnect and try again.
        </p>
        <Button size="sm" variant="secondary" onClick={retry}>
          Try again
        </Button>
      </>
    );
  }
  return (
    <output>
      <LoaderCircle size={14} className="animate-spin motion-reduce:animate-none" />
      Loading connection options…
    </output>
  );
}

function LoginProgress({ controller, name }: { controller: ConnectionController; name: string }) {
  const { pending, flow, authorizationUrl, copied, setCopied, setError } = controller;
  return (
    <>
      {pending && (
        <output className="agent-tool-login-progress">
          <LoaderCircle size={14} className="animate-spin motion-reduce:animate-none" />
          <span>{nonEmptyText(flow?.message) ?? "Waiting for you to finish signing in…"}</span>
        </output>
      )}
      {hasText(authorizationUrl) && pending && (
        <a
          className="agent-tool-browser-link"
          href={authorizationUrl}
          target="_blank"
          rel="noreferrer"
        >
          Open sign-in page
          <ExternalLink size={13} />
        </a>
      )}
      {hasText(flow?.userCode) && pending && (
        <div className="onboarding-command">
          <code>{flow?.userCode}</code>
          <Button
            variant="tertiary"
            size="sm"
            aria-label={`Copy ${name} sign-in code`}
            onClick={() => {
              const code = flow?.userCode;
              if (!hasText(code)) {
                return;
              }
              startAction(
                copyText(code).then(() => {
                  setCopied(true);
                }),
                errorReporter(setError),
              );
            }}
          >
            {copied ? <Check size={13} /> : <Copy size={13} />}
            {copied ? "Copied" : "Copy"}
          </Button>
        </div>
      )}
    </>
  );
}

function CredentialStorage({
  controller,
  method,
  name,
  harness,
  nodeName,
}: {
  controller: ConnectionController;
  method: NonNullable<ConnectionController["method"]>;
  name: string;
  harness: string;
  nodeName: string;
}) {
  const { storesCredential, pending, id, storage, setStorage, setup, busy } = controller;
  return storesCredential && !pending ? (
    <>
      <label htmlFor={`${id}-storage`} className="agent-tool-field">
        Credential storage
        <Select
          aria-label={`${name} credential storage`}
          value={storage}
          onChange={(key) => {
            if (typeof key === "string") {
              ((value) => {
                if (value === "secure" || value === "session") {
                  setStorage(value);
                }
              })(key);
            }
          }}
          isDisabled={busy}
          placeholder="Select…"
        >
          <Select.Trigger id={`${id}-storage`}>
            <Select.Value />
            <Select.Indicator />
          </Select.Trigger>
          <Select.Popover>
            <ListBox aria-label={`${name} credential storage`}>
              {[
                {
                  id: "secure",
                  label: "Secure storage",
                  disabled: !setup.storage.persistent,
                },
                { id: "session", label: "This runtime session only" },
              ].map((option) => (
                <ListBox.Item
                  key={option.id}
                  id={option.id}
                  textValue={option.label}
                  isDisabled={"disabled" in option && option.disabled === true}
                >
                  {"icon" in option && isValidElement(option.icon) ? option.icon : null}
                  {option.label}
                  <ListBox.ItemIndicator />
                </ListBox.Item>
              ))}
            </ListBox>
          </Select.Popover>
        </Select>
      </label>
      <p className="agent-tool-storage-note">
        {credentialStorageLabel(storage, harness, method.kind, nodeName, setup)} Swarmie never saves
        credentials in this browser.
      </p>
    </>
  ) : null;
}

function credentialStorageLabel(
  storage: string,
  harness: string,
  kind: string,
  nodeName: string,
  setup: AgentToolSetup,
) {
  if (storage === "secure") {
    if (harness === "cursor" && kind === "browser") {
      return `Your Cursor API key is saved in secure storage on ${nodeName}.`;
    }
    return (
      nonEmptyText(setup.storage.description) ??
      `Your key is saved in secure storage on ${nodeName}.`
    );
  }
  return setup.storage.persistent
    ? "Your key is kept in memory until this runtime stops."
    : "Secure storage is unavailable on this computer. Your key is kept in memory until this runtime stops.";
}
function connectionActionLabel(pending: boolean, kind: string) {
  if (pending) {
    return "Continue sign-in";
  }
  if (kind === "existing") {
    return "Check existing login";
  }
  return kind === "api_key" ? "Connect" : "Start sign-in";
}

function selectedProvider(setup: AgentToolSetup, id?: string) {
  return (
    setup.providers.find((candidate) => candidate.id === (id ?? setup.connection?.provider)) ??
    setup.providers.at(0)
  );
}
function selectedMethod(
  setup: AgentToolSetup,
  provider: ReturnType<typeof selectedProvider>,
  id: string,
) {
  return (
    provider?.methods.find((candidate) => candidate.id === id) ??
    preferredConnectionMethod(provider, setup)
  );
}
