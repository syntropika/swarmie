// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import Ajv from "ajv/dist/2020.js";

import schema from "./protocol.json" with { type: "json" };

const engine = new Ajv({ strict: false, validateFormats: false });
const validators = new Map();

function decode(key, resultSchema, value) {
  let validate = validators.get(key);
  if (!validate) {
    validate = engine.compile({ ...resultSchema, $defs: schema.$defs });
    validators.set(key, validate);
  }
  if (!validate(value)) {
    throw new TypeError(`Invalid wire response for ${key}.`);
  }
  return value;
}

/** Method-selected schemas prevent a caller's type parameter from validating unrelated data. */
export function decodeRuntimeResult(method, value) {
  if (!Object.hasOwn(schema["x-methods"], method)) {
    throw new TypeError("Unknown runtime method.");
  }
  return decode(`method:${method}`, schema["x-methods"][method].result, value);
}

/** Native replay and private projections reuse the runtime-owned definition schemas. */
export function decodeContract(name, value) {
  if (!Object.hasOwn(schema.$defs, name)) {
    throw new TypeError("Unknown contract definition.");
  }
  return decode(`definition:${name}`, { $ref: `#/$defs/${name}` }, value);
}
