import type { RuntimeEvent } from "@swarmie/contracts";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { AppState } from "react-native";

import { useRuntime } from "~/src/connection/provider";
import { appendEvents } from "~/src/conversations/journal";

export function useTranscript(sessionId: string) {
  const connection = useRuntime();
  const cache = useQueryClient();
  const queryKey = ["transcript", connection.scope, sessionId];
  const query = useQuery({
    queryKey,
    queryFn: async ({ signal }) => {
      let events: RuntimeEvent[] = [];
      for (;;) {
        // oxlint-disable-next-line no-await-in-loop -- Journal pages depend on the previous sequence.
        const next = await connection.client.call(
          "session/transcript",
          { sessionId, afterSequence: events.at(-1)?.sequence ?? 0, limit: 256 },
          signal,
        );
        events = appendEvents(events, next);
        if (next.length < 256) {
          return events;
        }
      }
    },
    enabled: sessionId.length > 0,
    staleTime: Infinity,
  });

  useEffect(() => {
    if (!query.isSuccess || query.fetchStatus !== "idle") {
      return undefined;
    }
    let controller = new AbortController();
    const key = ["transcript", connection.scope, sessionId];
    async function stream(signal: AbortSignal) {
      try {
        const retained = cache.getQueryData<RuntimeEvent[]>(key) ?? [];
        for await (const event of connection.client.events(
          sessionId,
          retained.at(-1)?.sequence ?? 0,
          signal,
        )) {
          cache.setQueryData<RuntimeEvent[]>(key, (current = []) => appendEvents(current, [event]));
          if (event.type === "state.changed" || event.type === "turn.ended") {
            await cache.invalidateQueries({ queryKey: ["sessions"] });
          }
        }
      } catch {
        if (!signal.aborted) {
          // Expired native deadlines and disconnects both recover from a retained journal cursor.
          await cache.invalidateQueries({ queryKey: key });
        }
      }
    }
    function start() {
      stream(controller.signal).catch(() => {
        console.warn("Transcript refresh failed");
      });
    }
    if (AppState.currentState === "active") {
      start();
    }
    const listener = AppState.addEventListener("change", (state) => {
      controller.abort();
      if (state === "active") {
        controller = new AbortController();
        start();
      }
    });
    return () => {
      controller.abort();
      listener.remove();
    };
  }, [connection, sessionId, cache, query.isSuccess, query.fetchStatus]);

  return query;
}
