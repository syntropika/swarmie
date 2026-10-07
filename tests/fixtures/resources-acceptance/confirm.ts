import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "@earendil-works/pi-ai";

/** Real native interactive input for isolated workflow acceptance. */
export default function (pi: ExtensionAPI) {
  pi.registerTool({
    name: "fixture_confirm",
    label: "Fixture confirmation",
    description: "Ask for explicit confirmation before returning a controlled fixture result.",
    parameters: Type.Object({}),
    async execute(_id, _params, _signal, _update, ctx) {
      const confirmed = await ctx.ui.confirm("Fixture permission", "Allow the controlled fixture operation?");
      return { content: [{ type: "text", text: confirmed ? "NATIVE_CONFIRM_YES" : "NATIVE_CONFIRM_NO" }], details: { confirmed } };
    },
  });
}
