import { expect, test } from "bun:test";

import { loadToolDetails } from "@swarmie/plugin-browser/settings";
import type { ToolDetails } from "@swarmie/plugin-browser/settings";
import { createHost } from "@syntropika/tembra-sdk/node";

import { initializeClient } from "../src/client/initialize";

test("invalid or unavailable local preferences fall back to grouped results", () => {
  for (const value of [undefined, null, "other", {}, false]) {
    expect(loadToolDetails({ load: () => value, save: () => undefined })).toBe("grouped");
  }
  expect(
    loadToolDetails({
      load: () => {
        throw new Error("Storage blocked");
      },
      save: () => undefined,
    }),
  ).toBe("grouped");
});

test("one client-wide display choice persists without modifying conversation state", async () => {
  let saved: ToolDetails = "separate";
  const persistence = {
    load: () => saved,
    save: (value: ToolDetails) => {
      saved = value;
    },
  };
  const first = await initializeClient(createHost(), {
    dev: false,
    toolDetailsPersistence: persistence,
  });
  try {
    expect(first.settings.getSnapshot().toolDetails).toBe("separate");
    const before = JSON.stringify(first.conversationState.getSnapshot());
    first.settings.update((state) => ({ ...state, toolDetails: "hidden" }));
    await Bun.sleep(10);
    expect(persistence.load()).toBe("hidden");
    expect(JSON.stringify(first.conversationState.getSnapshot())).toBe(before);
  } finally {
    await first.close();
  }
  const second = await initializeClient(createHost(), {
    dev: false,
    toolDetailsPersistence: persistence,
  });
  try {
    expect(second.settings.getSnapshot().toolDetails).toBe("hidden");
  } finally {
    await second.close();
  }
});
