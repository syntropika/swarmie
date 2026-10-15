import { z } from "zod";

export const routineSchema = z.object({
  id: z.string(),
  sessionId: z.string(),
  text: z.string(),
  nextRunAtMs: z.number(),
  intervalSeconds: z.number().nullable(),
  enabled: z.boolean(),
  status: z.string(),
  lastRunAtMs: z.number().nullable(),
  lastResult: z.unknown(),
  error: z.string().nullable(),
  attempts: z
    .array(
      z.object({
        id: z.string(),
        startedAtMs: z.number(),
        finishedAtMs: z.number().nullable(),
        status: z.string(),
        result: z.unknown(),
      }),
    )
    .default([]),
});
export const routinesSchema = z.object({
  schedules: z.array(routineSchema),
  lastError: z.string().nullable(),
});

/** Current runtime schedules are absolute instants with fixed elapsed-time intervals. */
export function scheduleInstant(value: string): number {
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(?::\d{2})?(?:Z|[+-]\d{2}:\d{2})$/u.test(value)) {
    throw new Error("Include a timezone offset or Z");
  }
  const instant = Date.parse(value);
  if (!Number.isFinite(instant) || instant <= Date.now()) {
    throw new Error("Choose a future time");
  }
  return instant;
}
