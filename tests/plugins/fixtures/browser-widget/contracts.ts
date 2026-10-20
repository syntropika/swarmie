import { defineHook, defineRegistry, local, method, Type } from "@tembra/sdk";

import type { RemoteBrowserAccessMainActivePackageResult } from "../../../../target/generated/contracts/remote-browser-access";

export type WidgetContribution =
  RemoteBrowserAccessMainActivePackageResult["manifest"]["ui"][number];

export const UiContributions = defineRegistry<{
  readonly plugin_id: string;
  readonly contribution: WidgetContribution;
}>("swarmie.ui-contributions");

export const Decorate = defineHook("swarmie.acceptance.decorate", {
  version: 1,
  strategy: "waterfall",
  input: Type.String(),
});

export const WidgetService = local("swarmie.acceptance.widget", {
  version: 1,
  input: Type.Object({}),
  members: {
    count: Type.Integer({ minimum: 0 }),
    increment: method(undefined, Type.Integer({ minimum: 0 })),
    decorate: Decorate.method,
  },
});

export const FixtureLifetime = { opened: 0, closed: 0 };
