import type { Thread } from "~/web/src/protocol";

export function watchThreadTimes(threads: Thread[], update: (now: number) => void) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let stopped = false;
  const tick = () => {
    if (stopped) {
      return;
    }
    const now = Date.now();
    update(now);
    const next = Math.min(
      ...threads
        .flatMap((thread) => {
          const timestamp = thread.updatedAtMs ?? thread.createdAtMs ?? 0;
          return [thread.snoozeUntil ?? 0, timestamp + 86_400_000, timestamp + 7 * 86_400_000];
        })
        .filter((deadline) => deadline > now),
    );
    if (Number.isFinite(next)) {
      timer = setTimeout(tick, Math.min(2_147_483_647, next - now + 5));
    }
  };
  tick();
  return () => {
    stopped = true;
    clearTimeout(timer);
  };
}
