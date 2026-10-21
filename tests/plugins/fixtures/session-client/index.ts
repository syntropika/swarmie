import { defineFacet, Type } from "@tembra/sdk";

import { DynamicConsumer } from "../../../../target/generated/contracts/dynamic-consumer";
import { SessionStore } from "../../../../target/generated/contracts/session-store";
import { serveGenerated } from "../support/generated-provider";

/** Exercises the generated Rust contract through the actual JavaScript executor. */
export default defineFacet({
  name: "session-client",
  uses: [SessionStore, DynamicConsumer],
  setup(_scope, declaration) {
    serveGenerated(
      declaration,
      DynamicConsumer,
      (_input, self) => {
        const store = self.get(SessionStore, {});
        const invocations = self.cell("invocations", {
          schema: Type.Integer({ minimum: 0, maximum: 4_294_967_295 }),
          initial: 0,
        });
        return {
          state: { invocations },
          methods: {
            async run(context) {
              await store.create(
                { session_id: "typescript", metadata: { consumer: "generated" } },
                context,
              );
              const request = {
                session_id: "typescript",
                batch_id: "typescript:message:1",
                expected_head: 0,
                events: [
                  {
                    event_id: "typescript:message:1",
                    kind: "swarmie.message",
                    version: 1,
                    correlation: {
                      conversation_id: "typescript:conversation",
                      turn_id: "typescript:turn",
                    },
                    payload: {
                      role: "user",
                      content: [{ type: "text", text: "Hello from TypeScript" }],
                      reply_to: null,
                      tool_call_id: null,
                    },
                  },
                ],
              };
              const receipt = await store.append(request, context);
              const retry = await store.append(request, context);
              if (receipt.last_sequence !== retry.last_sequence) {
                throw new Error("Idempotent receipt changed");
              }
              await store.append(
                {
                  session_id: "typescript",
                  batch_id: "typescript:other",
                  events: [
                    {
                      event_id: "typescript:other",
                      kind: "swarmie.message",
                      version: 1,
                      correlation: { conversation_id: "typescript:other" },
                      payload: {
                        role: "user",
                        content: "Other conversation",
                        reply_to: null,
                        tool_call_id: null,
                      },
                    },
                  ],
                },
                context,
              );
              const status = await store.publication_status(context);
              if (
                status.pending ||
                status.durable.commit_sequence !== status.published.commit_sequence
              ) {
                throw new Error("Session watermark is not current");
              }
              const page = await store.project(
                {
                  kind: "conversation",
                  source: {
                    session_id: "typescript",
                    conversation_id: "typescript:conversation",
                    after_sequence: 0,
                    limit: 128,
                    max_bytes: 786_432,
                  },
                },
                context,
              );
              if (page.entries.length !== 1 || page.uninterpreted !== 0) {
                throw new Error("Transcript projection lost data");
              }
              invocations.replace(context, invocations.value + 1);
              return page.entries.length;
            },
          },
        };
      },
      { lifetime: "host" },
    );
  },
});
