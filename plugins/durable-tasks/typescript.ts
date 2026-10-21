import type { Completion, ManagedContext, ObjectDeclaration } from "@tembra/sdk";

import type { TaskHandlerReference } from "../../target/generated/contracts/task-handler";

export type TaskInvocation = Parameters<TaskHandlerReference["step"]>[0];
export type TaskTransition = Awaited<ReturnType<TaskHandlerReference["step"]>>;

export type TaskDefinition = {
  readonly kind: string;
  readonly version: number;
  readonly step: (
    context: ManagedContext,
    invocation: TaskInvocation,
  ) => Completion<TaskTransition>;
};

/** Registers one application-owned operation in Tembra; it creates no execution runtime. */
export function defineTask(definition: TaskDefinition) {
  const { kind, version, step } = definition;
  if (
    !/^[A-Za-z0-9_.:-]{1,128}$/u.test(kind) ||
    !Number.isSafeInteger(version) ||
    version < 1 ||
    version > 4_294_967_295
  ) {
    throw new TypeError("Task definitions require a stable kind and a positive u32 version");
  }
  return (_input: unknown, self: ObjectDeclaration) => {
    self.registerMethod(
      { name: "swarmie.task-definitions", conflict: "error" },
      kind,
      { kind, version },
      "step",
    );
    return { state: {}, methods: { step } };
  };
}

export { TaskLifecycle, type TaskLifecycleEvent } from "./lifecycle-typescript";
