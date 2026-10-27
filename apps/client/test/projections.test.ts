import { expect, test } from "bun:test";

import { selectSnapshot } from "../src/client/projections";

test("a selected primitive stays unchanged when unrelated owner data changes", () => {
  let current = { text: "Draft", model: "balanced" };
  const source = { getSnapshot: () => current, subscribe: () => () => undefined };
  const model = selectSnapshot(source, (value) => value.model);
  expect(model.getSnapshot()).toBe("balanced");
  current = { ...current, text: "Another word" };
  expect(model.getSnapshot()).toBe("balanced");
  current = { ...current, model: "fast" };
  expect(model.getSnapshot()).toBe("fast");
});

test("a selected object retains identity until one of its visible fields changes", () => {
  let current = { text: "Draft", model: "balanced", effort: "Medium" };
  let reads = 0;
  const source = { getSnapshot: () => current, subscribe: () => () => undefined };
  const selected = selectSnapshot(
    source,
    ({ model, effort }) => {
      reads += 1;
      return { model, effort };
    },
    (a, b) => a.model === b.model && a.effort === b.effort,
  );
  const first = selected.getSnapshot();
  expect(selected.getSnapshot()).toBe(first);
  expect(reads).toBe(1);
  current = { ...current, text: "Typing" };
  expect(selected.getSnapshot()).toBe(first);
  expect(selected.getSnapshot()).toBe(first);
  expect(reads).toBe(2);
  current = { ...current, effort: "High" };
  const next = selected.getSnapshot();
  expect(next).not.toBe(first);
  expect(next).toEqual({ model: "balanced", effort: "High" });
  expect(selected.getSnapshot()).toBe(next);
});

test("selected subscriptions follow the owner and release their listeners", () => {
  const listeners = new Set<() => void>();
  let current = { value: 1 };
  const source = {
    getSnapshot: () => current,
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
  const selected = selectSnapshot(source, (state) => state.value);
  const observations: number[] = [];
  const detach = selected.subscribe(() => {
    observations.push(selected.getSnapshot());
  });
  current = { value: 2 };
  for (const listener of listeners) {
    listener();
  }
  expect(observations).toEqual([2]);
  detach();
  expect(listeners.size).toBe(0);
});
