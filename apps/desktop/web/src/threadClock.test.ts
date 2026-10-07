import { afterEach, expect, it, vi } from "vitest";
import { newThread } from "./protocol";
import { watchThreadTimes } from "./threadClock";

afterEach(() => vi.useRealTimers());

it("wakes for consecutive snoozes and time groups without any store mutation", () => {
  vi.useFakeTimers();
  vi.setSystemTime(0);
  const threads = [100, 200].map((snoozeUntil) => ({
    ...newThread("project", "workspace", "pi"),
    createdAtMs: 0,
    updatedAtMs: 0,
    snoozeUntil,
  }));
  const update = vi.fn();
  const stop = watchThreadTimes(threads, update);
  vi.advanceTimersByTime(105);
  expect(update).toHaveBeenLastCalledWith(105);
  expect(
    threads.filter((thread) => thread.snoozeUntil <= update.mock.lastCall![0]),
  ).toHaveLength(1);
  vi.advanceTimersByTime(100);
  expect(update).toHaveBeenLastCalledWith(205);
  expect(
    threads.filter((thread) => thread.snoozeUntil <= update.mock.lastCall![0]),
  ).toHaveLength(2);
  vi.advanceTimersByTime(86400_000 - 200);
  expect(update).toHaveBeenLastCalledWith(86400_005);
  vi.advanceTimersByTime(6 * 86400_000);
  expect(update).toHaveBeenLastCalledWith(7 * 86400_000 + 5);
  stop();
  expect(vi.getTimerCount()).toBe(0);
});

it("bounds long timers and cancels the pending wake on cleanup", () => {
  vi.useFakeTimers();
  vi.setSystemTime(0);
  const update = vi.fn();
  const thread = {
    ...newThread("project", "workspace", "pi"),
    createdAtMs: -8 * 86400_000,
    updatedAtMs: -8 * 86400_000,
    snoozeUntil: 3_000_000_000,
  };
  const stop = watchThreadTimes([thread], update);
  vi.advanceTimersByTime(2_147_483_647);
  expect(update).toHaveBeenLastCalledWith(2_147_483_647);
  stop();
  vi.advanceTimersByTime(3_000_000_000);
  expect(update).toHaveBeenCalledTimes(2);
});
