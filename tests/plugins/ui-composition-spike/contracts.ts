import { defineRegistry, local, method, Type } from "@tembra/sdk";

/** Synthetic presentation contracts for this spike; no production UI API is introduced. */
export const Surfaces = defineRegistry<{ readonly cardinality: "one" | "many" }>(
  "swarmie.spike.surfaces",
  { conflict: "error" },
);

export type Contribution =
  | {
      readonly kind: "screen";
      readonly surface: string;
      readonly label: string;
      readonly module: string;
      readonly export: string;
      readonly generation: number;
    }
  | {
      readonly kind: "action";
      readonly surface: string;
      readonly label: string;
      readonly generation: number;
    };

export const Contributions = defineRegistry<Contribution>("swarmie.spike.contributions", {
  conflict: "error",
});

export const Snapshot = Type.Object({
  count: Type.Integer({ minimum: 0 }),
  generation: Type.Integer({ minimum: 1 }),
  serverResult: Type.Union([Type.Null(), Type.Integer({ minimum: 0 })]),
});

export type CounterSnapshot = {
  readonly count: number;
  readonly generation: number;
  readonly serverResult: number | null;
};

export const Counter = local("swarmie.spike.counter", {
  version: 1,
  input: Type.Object({}),
  members: { snapshot: Snapshot, increment: method(undefined, Snapshot) },
});

export const Shell = local("swarmie.spike.shell", {
  version: 1,
  input: Type.Object({}),
  members: { mounted: Type.Boolean() },
});

export const Lifetime = { opened: 0, closed: 0 };

export type Observation<T> = {
  readonly getSnapshot: () => T;
  readonly subscribe: (listener: () => void) => () => void;
};
