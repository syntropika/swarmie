import { defineFacet, hookHandler, registerHook, Type } from "@tembra/sdk";

import { Decorate, FixtureLifetime, UiContributions, WidgetService } from "./contracts";

/** Runtime ownership stays in the Browser Host; the module declares only its contributions. */
export function widgetFacet(version: number) {
  return defineFacet({
    name: "browser-widget",
    uses: [WidgetService],
    setup(_scope, declaration) {
      declaration.serve(
        WidgetService,
        (_input, self) => {
          const count = self.cell("count", {
            schema: Type.Integer({ minimum: 0 }),
            initial: 0,
          });
          self.resource("lifetime", {
            open() {
              FixtureLifetime.opened += 1;
              return {};
            },
            close() {
              FixtureLifetime.closed += 1;
            },
          });
          self.register(UiContributions, "widget", {
            plugin_id: "multi-host-widget",
            contribution: {
              id: "widget",
              plugin_id: "multi-host-widget",
              kind: "panel",
              slot_id: "acceptance",
              order: 0,
              module_path: "widget.js",
              export_name: "Widget",
              label: `Counter view ${version}`,
            },
          });
          registerHook(self, Decorate, "widget", "decorate");
          return {
            state: { count },
            methods: {
              increment(context) {
                count.replace(context, count.value + version);
                return count.value;
              },
              decorate: hookHandler(Decorate, (_context, args) => `${args.input}:${version}`),
            },
          };
        },
        { lifetime: "host" },
      );
    },
  });
}
