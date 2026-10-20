import { defineFacet, Type } from "@tembra/sdk";

import { ConfigurationControl } from "../../../../target/generated/contracts/configuration-control";
import { MemoryStore } from "../../../../target/generated/contracts/memory-store";
import { PluginConfiguration } from "../../../../target/generated/contracts/plugin-configuration";
import { serveGenerated } from "../support/generated-provider";

const encoder = new TextEncoder();

function parseConfiguration(configuration: unknown) {
  if (
    configuration === null ||
    typeof configuration !== "object" ||
    Array.isArray(configuration) ||
    Object.keys(configuration).some((key) => key !== "initial")
  ) {
    throw new TypeError("Dynamic MemoryStore configuration permits only initial");
  }
  const initial: unknown = Reflect.get(configuration, "initial");
  return { initial: initial === undefined ? null : initial };
}

function validate(value: string, maximum: number): void {
  if (
    value.length === 0 ||
    encoder.encode(value).length > maximum ||
    /\p{Cc}/u.test(value) ||
    /^\p{White_Space}|\p{White_Space}$/u.test(value)
  ) {
    throw new TypeError("Invalid MemoryStore namespace or key");
  }
}

function compareUtf8(left: string, right: string): number {
  const a = encoder.encode(left);
  const b = encoder.encode(right);
  for (let index = 0; index < Math.min(a.length, b.length); index += 1) {
    const difference = (a[index] ?? 0) - (b[index] ?? 0);
    if (difference !== 0) {
      return difference;
    }
  }
  return a.length - b.length;
}

/** Ephemeral acceptance replacement using the owning plugin's canonical storage Contract. */
export function memoryStoreFacet(failAfterApply: boolean) {
  return defineFacet({
    name: "dynamic-memory-store",
    uses: [MemoryStore, ConfigurationControl, PluginConfiguration],
    setup(_scope, declaration) {
      serveGenerated(
        declaration,
        ConfigurationControl,
        (_input, self) => {
          const input = self.get(PluginConfiguration, { facet_id: "dynamic-memory-store" });
          const configuration = self.cell("configuration", {
            schema: Type.Object({ initial: Type.Unknown() }),
            async initialize(context) {
              const value = await input.read(context);
              return parseConfiguration(value.configuration);
            },
          });
          const registry = { name: "swarmie.configuration-controls", conflict: "error" } as const;
          for (const phase of ["validate", "apply"] as const) {
            self.registerMethod(
              registry,
              `dynamic-memory-store:${phase}`,
              { facet_id: "dynamic-memory-store", phase },
              phase,
            );
          }
          return {
            state: { configuration },
            methods: {
              validate(_context, candidate) {
                parseConfiguration(candidate);
                return undefined;
              },
              apply(context, candidate) {
                const next = parseConfiguration(candidate);
                configuration.replace(context, next);
                if (failAfterApply) {
                  throw new Error("injected_configuration_apply_failure");
                }
                return undefined;
              },
            },
          };
        },
        { lifetime: "host" },
      );
      serveGenerated(
        declaration,
        MemoryStore,
        (_input, self) => {
          const configuration = self.get(ConfigurationControl, {});
          const entries = self.resource("entries", {
            open: () => new Map<string, Map<string, unknown>>(),
            close(value) {
              value.clear();
            },
          });
          return {
            state: {},
            methods: {
              get(_context, request) {
                validate(request.namespace, 128);
                validate(request.key, 512);
                if (request.namespace === "configuration" && request.key === "initial") {
                  return { value: parseConfiguration(configuration.configuration.value).initial };
                }
                const namespace = entries.value.get(request.namespace);
                return namespace?.has(request.key) === true
                  ? { value: namespace.get(request.key) }
                  : null;
              },
              put(_context, request) {
                validate(request.namespace, 128);
                validate(request.key, 512);
                let namespace = entries.value.get(request.namespace);
                if (namespace === undefined) {
                  namespace = new Map();
                  entries.value.set(request.namespace, namespace);
                }
                namespace.set(request.key, request.value);
                return undefined;
              },
              delete(_context, request) {
                validate(request.namespace, 128);
                validate(request.key, 512);
                return entries.value.get(request.namespace)?.delete(request.key) ?? false;
              },
              list(_context, request) {
                validate(request.namespace, 128);
                return [...(entries.value.get(request.namespace)?.keys() ?? [])].toSorted(
                  compareUtf8,
                );
              },
            },
          };
        },
        { lifetime: "host" },
      );
    },
  });
}

export default memoryStoreFacet(false);
