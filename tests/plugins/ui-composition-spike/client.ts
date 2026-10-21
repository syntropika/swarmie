import { Button } from "@swarmie/ui-web";
import { createHost } from "@tembra/sdk/browser";
import type { BrowserWasmModule } from "@tembra/sdk/browser";
import { Component, createElement } from "react";
import type { ComponentType, ReactNode } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";

import { RemoteBrowserAccess } from "../../../target/generated/contracts/remote-browser-access";
import { Contributions, Counter, Lifetime, Surfaces } from "./contracts";
import type { CounterSnapshot, Observation } from "./contracts";

function requireValue<T>(value: T | undefined | null, message: string): T {
  if (value === undefined || value === null) {
    throw new Error(message);
  }
  return value;
}

/** A cache satisfies React identity requirements; Tembra still owns the observed value. */
function observation<T>(initial: T, attach: (listener: (value: T) => void) => () => void) {
  let cached = initial;
  const listeners = new Set<() => void>();
  let publications = 0;
  const unsubscribe = attach((value) => {
    cached = value;
    publications += 1;
    for (const listener of listeners) {
      listener();
    }
  });
  return {
    getSnapshot: () => cached,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    diagnostics: () => ({ publications, reactListeners: listeners.size }),
    close() {
      unsubscribe();
      listeners.clear();
    },
  };
}

class Boundary extends Component<{ children?: ReactNode }, { failed: boolean }> {
  constructor(props: { children?: ReactNode }) {
    super(props);
    this.state = { failed: false };
  }

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render(): ReactNode {
    return this.state.failed
      ? createElement(
          "p",
          { role: "alert", id: "renderer-error" },
          "This contribution failed. Other controls remain available.",
        )
      : this.props.children;
  }
}

type Screen = ComponentType<{ readonly model: Observation<CounterSnapshot> }>;
function isScreen(value: unknown): value is Screen {
  return typeof value === "function";
}

async function control(command: string, receipt?: unknown): Promise<unknown> {
  const response = await fetch("/control", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ command, receipt }),
  });
  if (!response.ok) {
    throw new Error(await response.text());
  }
  const result: unknown = await response.json();
  return result;
}

function isInitializer(value: unknown): value is () => Promise<void> {
  return typeof value === "function";
}

function stage(value: string) {
  document.documentElement.dataset.stage = value;
}

async function main() {
  stage("create-host");
  const host = await createHost({
    name: "ui-composition-spike",
    wasm: async () => {
      const loaded: unknown = await import(
        new URL("/wasm/tembra_browser_wasm.js", location.href).href
      );
      if (
        loaded === null ||
        typeof loaded !== "object" ||
        !("default" in loaded) ||
        !isInitializer(loaded.default) ||
        !("WasmHost" in loaded) ||
        !("createCheckedWasmExecutor" in loaded)
      ) {
        throw new Error("Matching Browser Host binding is unavailable");
      }
      await loaded.default();
      // SAFETY: These exact generated binding bytes and WASM ABI are verified by the pinned build provenance.
      // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- The fixture serves the provenance-verified matching generated WASM ABI.
      return loaded as BrowserWasmModule;
    },
  });
  const bootstrapResponse = await fetch("/bootstrap");
  const bootstrap: unknown = await bootstrapResponse.json();
  if (
    bootstrap === null ||
    typeof bootstrap !== "object" ||
    !("address" in bootstrap) ||
    typeof bootstrap.address !== "string"
  ) {
    throw new Error("Invalid fixture bootstrap");
  }
  const connection = host.connect(`ws://${bootstrap.address}`, "swarmie-runtime");
  stage("connect");
  await connection.ready();
  stage("artifact-access");
  const access = await host.get(RemoteBrowserAccess, {});
  const registry = host.registry(Contributions);
  const surfaces = host.registry(Surfaces);
  const root = createRoot(
    requireValue(document.querySelector("#app"), "Missing application mount"),
  );
  let selection = await access.active_package({ plugin_id: "multi-host-widget" });
  stage("package-selected");
  let reference: Awaited<ReturnType<typeof acquireCounter>> | undefined;
  let model: ReturnType<typeof observation<CounterSnapshot>> | undefined;
  let closedModel = { publications: 0, reactListeners: 0 };
  let screen: Screen | undefined;
  let screenLabel = "";
  let resolution = 0;
  let staleResolutions = 0;
  let observerInstalls = 0;
  let busy = false;
  let failure = "";
  let installed = false;
  let retired = false;
  let shellPresent = true;
  let closed = false;
  const urls = new Set<string>();

  async function acquireCounter() {
    return await host.get(Counter, {});
  }

  const report = async (observed: "active" | "inactive", current = selection) =>
    await control("report", {
      plugin_id: current.manifest.id,
      version: current.manifest.version,
      activation_id: current.activation_id,
      observed,
      error: "",
    });

  async function moduleUrl(file: string) {
    const expected = requireValue(
      selection.manifest.files.find((value) => value.path === file),
      "Unapproved module",
    );
    const chunks: number[] = [];
    let total = 0;
    do {
      // oxlint-disable-next-line no-await-in-loop -- Approved chunks require the exact preceding offset.
      const part = await access.read_package_file({
        plugin_id: selection.manifest.id,
        activation_id: selection.activation_id,
        path: file,
        offset: chunks.length,
        length: 32_768,
      });
      if ((total !== 0 && total !== part.total_length) || part.bytes.length === 0) {
        throw new Error("Invalid module chunk");
      }
      total = part.total_length;
      chunks.push(...part.bytes);
      if (chunks.length > total) {
        throw new Error("Module length exceeded");
      }
    } while (chunks.length < total);
    const bytes = Uint8Array.from(chunks);
    const hash = Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)), (value) =>
      value.toString(16).padStart(2, "0"),
    ).join("");
    if (hash !== expected.sha256) {
      throw new Error("Module integrity mismatch");
    }
    const url = URL.createObjectURL(new Blob([bytes], { type: "text/javascript" }));
    urls.add(url);
    return url;
  }

  function diagnostics() {
    return {
      installed,
      retired,
      shellPresent,
      version: selection.manifest.version,
      contributions: registry.table().entries.map(([id]) => id),
      surfaces: surfaces.table().entries.map(([id]) => id),
      referenceStatus: reference?.status ?? "not-acquired",
      snapshot: model?.getSnapshot() ?? null,
      observerInstalls,
      observation: model?.diagnostics() ?? closedModel,
      staleResolutions,
      resources: { ...Lifetime },
      host: host.diagnostics(),
    };
  }

  async function invoke(operation: () => Promise<unknown>) {
    busy = true;
    failure = "";
    render();
    try {
      await operation();
    } catch (error) {
      failure = String(error);
    } finally {
      busy = false;
      render();
    }
  }

  const button = (id: string, label: string, action: () => Promise<unknown>, disabled = false) =>
    createElement(
      Button,
      {
        id,
        variant: "secondary",
        isDisabled: busy || disabled,
        onPress: () => {
          invoke(action).catch(console.error);
        },
      },
      label,
    );

  function render() {
    if (closed) {
      return;
    }
    const actions = registry
      .table()
      .entries.filter(
        ([, entry]) =>
          entry.data.kind === "action" && surfaces.table().get(entry.data.surface) !== undefined,
      );
    flushSync(() => {
      root.render(
        createElement(
          "div",
          { className: "mx-auto max-w-5xl space-y-8 p-8" },
          createElement(
            "header",
            { className: "space-y-2" },
            createElement("h1", { className: "text-3xl font-semibold" }, "UI composition spike"),
            createElement(
              "p",
              { className: "text-muted" },
              "Real Tembra runtime · independent shell and feature · React renderer",
            ),
          ),
          createElement(
            "section",
            { "aria-label": "Spike controls", className: "flex flex-wrap gap-3" },
            button("install", "Install UI plugin", install, installed || retired),
            button(
              "update",
              "Update plugin",
              update,
              !installed || selection.manifest.version !== "1.0.0",
            ),
            button("retire", "Retire plugin", retire, !installed),
            button("shell", shellPresent ? "Remove shell" : "Restore shell", toggleShell),
            button("fault", "Fail renderer", failRenderer, !installed || !shellPresent),
            button("recover", "Recover renderer", refresh, !installed || !shellPresent),
          ),
          failure ? createElement("p", { role: "alert" }, failure) : null,
          createElement(
            "div",
            { className: "grid gap-8 md:grid-cols-[14rem_1fr]" },
            createElement(
              "nav",
              { "aria-label": "Plugin screens" },
              screen && shellPresent
                ? createElement(
                    "p",
                    { id: "screen-label", className: "font-semibold" },
                    screenLabel,
                  )
                : createElement("p", { className: "text-muted" }, "No available screen"),
            ),
            createElement(
              "main",
              { className: "space-y-6" },
              createElement(
                "div",
                { "aria-label": "Contributed actions", className: "flex flex-wrap gap-3" },
                ...actions.map(([id, entry]) =>
                  button(id, entry.data.label, async () => {
                    await invokeAction(id);
                  }),
                ),
              ),
              screen && model && shellPresent
                ? createElement(Boundary, { key: resolution }, createElement(screen, { model }))
                : createElement(
                    "p",
                    { id: "empty" },
                    installed
                      ? "Feature contributions are dormant while the shell is absent."
                      : "Install the UI plugin to contribute a screen and an action.",
                  ),
            ),
          ),
          createElement(
            "section",
            { "aria-label": "Runtime evidence", className: "space-y-3" },
            createElement("h2", { className: "text-lg font-semibold" }, "Runtime evidence"),
            createElement(
              "pre",
              { id: "diagnostics", className: "overflow-x-auto rounded-lg bg-surface p-4 text-sm" },
              JSON.stringify(diagnostics(), null, 2),
            ),
          ),
        ),
      );
    });
  }

  async function resolveScreen(
    exportOverride?: string,
    gate?: { wait: Promise<void>; ready: () => void },
  ) {
    resolution += 1;
    const token = resolution;
    const entry = registry
      .table()
      .entries.find(
        ([, value]) =>
          value.data.kind === "screen" && surfaces.table().get(value.data.surface) !== undefined,
      );
    if (entry?.[1].data.kind !== "screen") {
      screen = undefined;
      render();
      return;
    }
    const [, { data }] = entry;
    const activation = selection.activation_id;
    const url = await moduleUrl(data.module);
    if (gate) {
      gate.ready();
      await gate.wait;
    }
    const loaded: unknown = await import(url);
    if (
      token !== resolution ||
      activation !== selection.activation_id ||
      !installed ||
      surfaces.table().get(data.surface) === undefined
    ) {
      staleResolutions += 1;
      return;
    }
    if (loaded === null || typeof loaded !== "object") {
      throw new Error("Invalid renderer namespace");
    }
    const component: unknown = Reflect.get(loaded, exportOverride ?? data.export);
    if (!isScreen(component)) {
      throw new Error("Missing renderer export");
    }
    screen = component;
    screenLabel = data.label;
    render();
  }

  async function refresh() {
    await resolveScreen();
  }
  async function failRenderer() {
    await resolveScreen("BrokenScreen");
  }

  async function install() {
    await host.load(await moduleUrl("facet.js"));
    const counter = await acquireCounter();
    reference = counter;
    model = observation(counter.snapshot.value, (listener) => counter.snapshot.subscribe(listener));
    observerInstalls += 1;
    installed = true;
    await refresh();
    await report("active");
    render();
  }

  async function invokeAction(id: string) {
    await host.root(async (context) => {
      const current = requireValue(registry.table(context).get(id), "Action is unavailable");
      await requireValue(current.operation, "Action has no managed operation").call(context, {
        present: false,
      });
    });
    render();
  }

  async function update() {
    const previous = selection;
    await control("update");
    selection = await access.active_package({ plugin_id: previous.manifest.id });
    await host.reload([await moduleUrl("facet.js")]);
    await refresh();
    await report("inactive", previous);
    await report("active");
    render();
  }

  async function retire() {
    await control("deactivate");
    installed = false;
    retired = true;
    resolution += 1;
    screen = undefined;
    await host.reload([], { remove: ["browser-widget"] });
    render();
    if (model) {
      closedModel = model.diagnostics();
      model.close();
    }
    await report("inactive");
    render();
  }

  async function toggleShell() {
    shellPresent = !shellPresent;
    await (shellPresent ? host.load("/shell.js") : host.reload([], { remove: ["spike.shell"] }));
    await refresh();
  }

  async function retireDuringResolution() {
    let resume: (() => void) | undefined;
    let fetched: (() => void) | undefined;
    // oxlint-disable-next-line promise/avoid-new -- A controlled fixture barrier completes an import after actual retirement.
    const gate = new Promise<void>((resolve) => {
      resume = resolve;
    });
    // oxlint-disable-next-line promise/avoid-new -- Confirm approved bytes are read before retirement revokes package delivery.
    const ready = new Promise<void>((resolve) => {
      fetched = resolve;
    });
    // Fetch approved bytes before retirement, but finish their import after retirement.
    const pending = resolveScreen("BrokenScreen", {
      wait: gate,
      ready: requireValue(fetched, "Missing fetch barrier"),
    });
    await ready;
    await retire();
    requireValue(resume, "Missing resolution barrier")();
    await pending;
    render();
  }

  const unsubscribeRegistry = registry.subscribe(() => {
    resolution += 1;
    screen = undefined;
    render();
  });
  const unsubscribeSurfaces = surfaces.subscribe(() => {
    resolution += 1;
    screen = undefined;
    render();
  });
  stage("load-shell");
  await host.load("/shell.js");
  render();
  stage("ready");
  document.documentElement.dataset.ready = "true";

  return {
    install,
    increment: async () => {
      await invokeAction("counter.increment");
    },
    update,
    retire,
    toggleShell,
    failRenderer,
    refresh,
    retireDuringResolution,
    diagnostics,
    snapshotStable() {
      if (!model) {
        return false;
      }
      const first = model.getSnapshot();
      const second = model.getSnapshot();
      return first === second;
    },
    async close() {
      closed = true;
      root.unmount();
      model?.close();
      unsubscribeRegistry();
      unsubscribeSurfaces();
      reference?.release();
      access.release();
      const final = diagnostics();
      await host.close();
      for (const url of urls) {
        URL.revokeObjectURL(url);
      }
      return { ...final, resources: { ...Lifetime }, host: host.diagnostics() };
    },
  };
}

const client = await main();
// A named fixture hook exposes only this local spike's lifecycle to the acceptance driver.
Object.assign(window, { uiSpike: client });
