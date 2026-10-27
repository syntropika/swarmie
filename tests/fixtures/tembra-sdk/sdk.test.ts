import { expect, test } from "bun:test";
import { createHost } from "@syntropika/tembra-sdk/node";

import counter from "./counter";

test("the published SDK executes an independent orb through its native engine", async () => {
  const host = createHost();
  try {
    await host.install(counter);
    const reference = await host.get(counter.services["fixture.sdk-counter"], "main");
    expect(await reference.increment(7)).toBe(7);
    expect(await reference.read()).toBe(7);
    reference.release();
  } finally {
    await host.close();
  }
});
