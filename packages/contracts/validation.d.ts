// Generated from runtime/schema. Run tools/generate.py to refresh.
import type { ContractDefinitions, RuntimeResults } from "./index.js";
export declare function decodeRuntimeResult<M extends keyof RuntimeResults>(
  method: M,
  value: unknown,
): RuntimeResults[M];
export declare function decodeContract<M extends keyof ContractDefinitions>(
  name: M,
  value: unknown,
): ContractDefinitions[M];
