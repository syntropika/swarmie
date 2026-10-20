import { defineFacet } from "@tembra/sdk";
import type { FacetScope } from "@tembra/sdk";
import { createHost } from "@tembra/sdk/browser";
import type { BrowserWasmModule } from "@tembra/sdk/browser";
import { createElement } from "react";
import type { ComponentType } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";

import { RemoteBrowserAccess } from "../../../target/generated/contracts/remote-browser-access";
import { RemoteCounter } from "../../../target/generated/contracts/remote-counter";
import {
  Decorate,
  FixtureLifetime,
  UiContributions,
  WidgetService,
} from "../fixtures/browser-widget/contracts";

function check(value: boolean, message: string): asserts value {
  if (!value) {
    throw new Error(message);
  }
}

function isWidget(value: unknown): value is ComponentType<{ label: string }> {
  return typeof value === "function";
}

async function refused(operation: () => Promise<unknown>, message: string): Promise<void> {
  let rejected = false;
  try {
    await operation();
  } catch {
    rejected = true;
  }
  check(rejected, message);
}

async function main() {
  const progress = document.querySelector("#result");
  const stage = (value: string) => {
    if (progress instanceof HTMLElement) {
      progress.dataset.stage = value;
    }
  };
  stage("bootstrap");
  const response = await fetch("/bootstrap");
  const bootstrap: unknown = await response.json();
  check(
    bootstrap !== null &&
      typeof bootstrap === "object" &&
      "address" in bootstrap &&
      typeof bootstrap.address === "string" &&
      "session_id" in bootstrap &&
      typeof bootstrap.session_id === "string",
    "validated supervisor bootstrap",
  );
  const host = await createHost({
    name: "plugin-browser-acceptance",
    wasm: async () => {
      // This generated module is served verbatim from the integrity-checked matching WASM build.
      const url = new URL("/wasm/tembra_browser_wasm.js", location.href);
      const loaded: unknown = await import(url.href);
      check(
        loaded !== null &&
          typeof loaded === "object" &&
          "default" in loaded &&
          typeof loaded.default === "function" &&
          "WasmHost" in loaded &&
          "createCheckedWasmExecutor" in loaded,
        "matching generated Browser Host binding",
      );
      // SAFETY: The build verifies this exact generated ABI against the pinned SDK and WASM hashes.
      // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- The generated ABI has the exact matching upstream Host and executor constructors.
      const module = loaded as BrowserWasmModule & { default: () => Promise<unknown> };
      await module.default();
      return module;
    },
  });
  stage("host-created");
  const connection = host.connect(`ws://${bootstrap.address}`, "swarmie-runtime");
  await connection.ready();
  stage("connected");
  const access = await host.get(RemoteBrowserAccess, {});
  stage("access-ready");
  const counter = await host.get(RemoteCounter, {});
  check((await counter.increment(1)) === (await counter.read()), "actual server RPC capability");
  stage("counter-ready");
  let selection = await access.active_package({ plugin_id: "multi-host-widget" });
  await refused(
    async () => await access.active_package({ plugin_id: "unapproved-plugin" }),
    "unavailable package is refused",
  );
  await refused(
    async () =>
      await access.read_package_file({
        plugin_id: selection.manifest.id,
        activation_id: selection.activation_id,
        path: "../unapproved.js",
        offset: 0,
        length: 1,
      }),
    "unlisted artifact path is refused",
  );
  const urls: string[] = [];
  async function approvedModule(file: string): Promise<string> {
    const expected = selection.manifest.files.find((candidate) => candidate.path === file);
    check(expected !== undefined, "module is listed in approved package");
    const chunks: number[] = [];
    let total: number | undefined;
    do {
      // oxlint-disable-next-line no-await-in-loop -- Each approved range depends on the preceding exact offset and total length.
      const result = await access.read_package_file({
        plugin_id: selection.manifest.id,
        activation_id: selection.activation_id,
        path: file,
        offset: chunks.length,
        length: 32_768,
      });
      check(total === undefined || total === result.total_length, "stable approved file length");
      total = result.total_length;
      check(
        result.bytes.length > 0 && chunks.length + result.bytes.length <= total,
        "bounded file progress",
      );
      chunks.push(...result.bytes);
    } while (chunks.length < total);
    const bytes = Uint8Array.from(chunks);
    const hash = Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)), (value) =>
      value.toString(16).padStart(2, "0"),
    ).join("");
    check(hash === expected.sha256, "approved module SHA-256 integrity");
    const url = URL.createObjectURL(new Blob([bytes], { type: "text/javascript" }));
    urls.push(url);
    return url;
  }
  type ActivePackage = typeof selection;
  const receipt = (
    observed: "active" | "inactive",
    packageSelection: ActivePackage = selection,
  ) => ({
    plugin_id: packageSelection.manifest.id,
    version: packageSelection.manifest.version,
    activation_id: packageSelection.activation_id,
    observed,
    error: "",
  });
  stage("package-authorized");
  host.registerHook(Decorate);
  let scope: FacetScope | undefined;
  await host.install(
    defineFacet({
      name: "hook-observer",
      uses: [],
      setup(current) {
        scope = current;
      },
    }),
  );
  const observer = scope;
  check(observer !== undefined, "issued hook observation scope");
  const format = async () =>
    await observer.root(async (context) => await observer.runHook(Decorate, context, "view"));
  stage("loading-browser-facet");
  await host.load(await approvedModule("facet.js"));
  const reference = await host.get(WidgetService, {});
  check(
    (await reference.increment()) === 1 && reference.count.value === 1,
    "real Browser Facet execution",
  );
  check((await format()) === "view:1", "owned browser Hook contribution");
  const mount = document.querySelector("#contribution");
  check(mount !== null, "React contribution mount");
  const container: Element = mount;
  let root = createRoot(container);
  async function render() {
    const entry = await host.root((context) =>
      host.registry(UiContributions).table(context).get("widget"),
    );
    check(entry !== undefined, "live UI Registry entry");
    const module: unknown = await import(await approvedModule(entry.data.contribution.module_path));
    check(module !== null && typeof module === "object", "native React module namespace");
    const component: unknown = Reflect.get(module, entry.data.contribution.export_name);
    check(isWidget(component), "selected React module export");
    flushSync(() => {
      root.render(
        createElement(component, {
          label: entry.data.contribution.label,
        }),
      );
    });
    check(
      container.textContent === entry.data.contribution.label,
      "React hooks share renderer module identity",
    );
  }
  stage("rendering");
  await render();
  await refused(async () => await host.reload(["/failing.js"]), "failed browser setup is refused");
  await refused(
    async () => await host.reload(["/incompatible.js"]),
    "incompatible browser replacement is refused",
  );
  check(
    (await reference.increment()) === 2 && (await format()) === "view:1",
    "refused replacement preserves actual runtime",
  );
  const installed = selection;
  return {
    installed: true,
    receipt: receipt("active"),
    async update() {
      await refused(
        async () =>
          await access.read_package_file({
            plugin_id: installed.manifest.id,
            activation_id: installed.activation_id,
            path: "facet.js",
            offset: 0,
            length: 1,
          }),
        "superseded receipt cannot address a different active release",
      );
      selection = await access.active_package({ plugin_id: "multi-host-widget" });
      await host.reload([await approvedModule("facet.js")]);
      check(
        reference.count.value === 2 && (await reference.increment()) === 4,
        "retained reference and State survive native ESM reload",
      );
      check((await format()) === "view:2", "new Hook generation is current");
      root.unmount();
      root = createRoot(container);
      await render();
      return {
        updated: true,
        count: reference.count.value,
        receipts: [receipt("inactive", installed), receipt("active")],
      };
    },
    async retire() {
      await refused(
        async () => await access.active_package({ plugin_id: selection.manifest.id }),
        "deactivated package delivery is refused",
      );
      await host.reload([], { remove: ["browser-widget"] });
      check(reference.status === "gone", "removed browser provider revokes reference");
      check((await format()) === "view", "retired Hook contribution is absent");
      const empty = await host.root(
        (context) => host.registry(UiContributions).table(context).get("widget") === undefined,
      );
      check(empty, "retired UI Registry has no stale contribution");
      root.unmount();
      check(container.textContent === "", "retired React contribution unmounts");
      reference.release();
      access.release();
      counter.release();
      await host.close();
      const diagnostics = host.diagnostics();
      check(
        diagnostics.callbacks === 0 && diagnostics.jobs === 0,
        "closed Host releases physical callbacks and jobs",
      );
      check(
        FixtureLifetime.opened === FixtureLifetime.closed,
        "managed resources close exactly once",
      );
      for (const url of urls) {
        URL.revokeObjectURL(url);
      }
      return {
        retired: true,
        resources: FixtureLifetime.closed,
        diagnostics,
        receipt: receipt("inactive"),
      };
    },
  };
}

declare global {
  // oxlint-disable-next-line typescript/consistent-type-definitions -- Extending the browser Window requires declaration merging.
  interface Window {
    acceptance: Awaited<ReturnType<typeof main>> | undefined;
  }
}

const output = document.querySelector("#result");
check(output !== null, "acceptance result output");
try {
  window.acceptance = await main();
  output.textContent = JSON.stringify({ installed: true, receipt: window.acceptance.receipt });
} catch (error) {
  output.textContent = JSON.stringify({
    failed: error instanceof Error ? error.message : String(error),
  });
}
