import {
  Type,
  defineFacet,
  hookFromDescriptor,
  hookHandler,
  local,
  registerHook,
} from "@tembra/sdk";
import type { ObjectDeclaration } from "@tembra/sdk";

import hookDescriptors from "../../../../target/generated/contracts/agent-hooks.json";

const contextDescriptor = hookDescriptors.find(
  (descriptor) => descriptor.name === "swarmie.agent.prepare-context",
);
if (contextDescriptor === undefined) {
  throw new Error("The canonical context hook descriptor is missing");
}
const prepareContext = hookFromDescriptor(contextDescriptor);
const ContextContribution = local("fixture.agent-context-contribution", {
  version: 1,
  input: Type.Object({}),
  members: { prepare: prepareContext.method },
});

/** Compatible generations contribute through Tembra ownership and real reload machinery. */
export function contextFacet(version: number) {
  const factory = (_input: unknown, self: ObjectDeclaration) => {
    registerHook(self, prepareContext, "js-context", "prepare", { priority: 50 });
    return {
      state: {},
      methods: {
        prepare: hookHandler(prepareContext, (_context, { input }) => {
          if (input === null || typeof input !== "object" || !Object.hasOwn(input, "messages")) {
            throw new TypeError("Expected a prepared context");
          }
          const messages: unknown = Reflect.get(input, "messages");
          if (!Array.isArray(messages)) {
            throw new TypeError("Expected context messages");
          }
          const existingMessages: readonly unknown[] = messages;
          return {
            ...input,
            messages: [
              ...existingMessages,
              {
                role: "system",
                content: [{ type: "text", text: `dynamic-context-v${version}` }],
              },
            ],
          };
        }),
      },
    };
  };
  return defineFacet({
    name: "fixture.agent-context",
    uses: [ContextContribution],
    setup(_scope, declaration) {
      // Peer descriptors intentionally carry unknown application types. Dispatch the real
      // serve method without inventing a second schema; Tembra checks its exact profile.
      Reflect.apply(declaration.serve.bind(declaration), declaration, [
        ContextContribution,
        factory,
        { lifetime: "host" },
      ]);
    },
  });
}
