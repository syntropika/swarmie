import runtimeSchema from "@swarmie/contracts/schema" with { type: "json" };
import type { ValidateFunction } from "ajv";
import Ajv from "ajv/dist/2020.js";

const engine = new Ajv({ strict: false, validateFormats: false });
const projections = new Map<string, string>();
let projectionVersion = 0;

/** Validate extension-owned projections before a controller updates UI state. */
type ProjectionSchema<T> = {
  type?: string;
  properties?: { [Key in keyof T]?: unknown };
  required?: (keyof T)[];
  items?: unknown;
  additionalProperties?: unknown;
  $ref?: string;
};

function projectionMatches<T>(validate: ValidateFunction<T>, value: unknown): value is T {
  const result: unknown = validate(value);
  return result === true;
}
export function extensionResult<T>(schema: ProjectionSchema<T>): (value: unknown) => T {
  const key = JSON.stringify(schema);
  let id = projections.get(key);
  if (id === undefined) {
    projectionVersion += 1;
    id = `urn:swarmie:extension-projection:${projectionVersion}`;
    engine.addSchema({ ...schema, $defs: runtimeSchema.$defs }, id);
    projections.set(key, id);
    if (projections.size > 128) {
      const oldest = projections.entries().next().value;
      if (oldest !== undefined) {
        engine.removeSchema(oldest[1]);
        projections.delete(oldest[0]);
      }
    }
  }
  const validate = engine.getSchema<T>(id);
  if (validate === undefined) {
    throw new TypeError("Extension response schema is unavailable.");
  }
  return (value) => {
    if (!projectionMatches(validate, value)) {
      throw new TypeError("Invalid extension response.");
    }
    return value;
  };
}
