import { afterEach, expect, test } from "bun:test";

import { bindState } from "@swarmie/client-ui";
import { settingsOrb } from "@swarmie/plugin-browser/settings";
import { orb } from "@syntropika/tembra-sdk";
import { createHost } from "@syntropika/tembra-sdk/node";

import { createPluginControls } from "../src/client/plugins";

let cleanup: (() => Promise<void>) | undefined;
afterEach(async () => {
  await cleanup?.();
  cleanup = undefined;
});

async function deferredActivation() {
  const host = createHost();
  const gate = Promise.withResolvers<undefined>();
  const errors: unknown[] = [];
  const counts = { acquired: 0, opened: 0, closed: 0 };
  const definition = orb({
    name: "fixture.optional",
    apiVersion: 1,
    services: {
      "fixture.optional": {
        create(self) {
          self.resource("contribution", {
            open: () => {
              counts.opened += 1;
              return {};
            },
            close: () => {
              counts.closed += 1;
            },
          });
          return {};
        },
        methods: {},
      },
    },
  });
  await host.install([settingsOrb, definition]);
  const owner = await host.get(settingsOrb.services["swarmie.settings.ui"], "main");
  function failed(error: unknown) {
    errors.push(error);
  }
  const state = bindState(owner, failed);
  async function acquire() {
    counts.acquired += 1;
    const reference = await host.get(definition.services["fixture.optional"], "main");
    await gate.promise;
    return reference;
  }
  const controls = createPluginControls(
    state,
    false,
    { projects: acquire, "read-aloud": acquire, "app-state": acquire, git: acquire },
    () => undefined,
    failed,
  );
  await owner.update((current) => ({
    ...current,
    plugins: current.plugins.map((plugin) =>
      plugin.id === "projects" ? { ...plugin, enabled: false } : plugin,
    ),
  }));
  cleanup = async () => {
    controls.close();
    owner.release();
    await host.close();
  };
  return { controls, gate, counts, errors };
}

test("racing enable actions retain one activation and disabling removes its contribution", async () => {
  const { controls, gate, counts, errors } = await deferredActivation();
  controls.togglePlugin("projects");
  controls.togglePlugin("projects");
  await Bun.sleep(5);
  gate.resolve(undefined);
  await Bun.sleep(5);
  expect(counts.acquired).toBe(1);
  expect(controls.plugins.getSnapshot().find((plugin) => plugin.id === "projects")?.enabled).toBe(
    true,
  );
  controls.togglePlugin("projects");
  await Bun.sleep(5);
  expect(counts.opened).toBe(1);
  expect(counts.closed).toBe(1);
  expect(errors).toEqual([]);
});

test("an activation completed after close releases its Reference and cannot reactivate controls", async () => {
  const { controls, gate, counts, errors } = await deferredActivation();
  const activation = controls.activate("projects");
  await Bun.sleep(5);
  controls.close();
  gate.resolve(undefined);
  await activation;
  controls.togglePlugin("projects");
  await Bun.sleep(5);
  expect(counts.acquired).toBe(1);
  expect(counts.opened).toBe(1);
  expect(counts.closed).toBe(1);
  expect(errors).toEqual([]);
});
