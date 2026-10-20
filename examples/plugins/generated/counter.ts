// Generated from a verified Tembra descriptor. Do not edit.

import { bindReference, fromDescriptor, parsePortableJson, type BindingPort, type ContextHandle, type ContractReference, type GeneratedContract, type Readable } from "@tembra/sdk";

export interface CounterInput {
  [k: string]: unknown | undefined;
}


export type CounterLocalCountState = number;


export type CounterLocalIncrementArgs = number;


export type CounterLocalIncrementResult = number;


export type CounterLocalReadResult = number;


export interface CounterLocalReference extends ContractReference {
  readonly variant: undefined;
  readonly "count": Readable<CounterLocalCountState>;
  readonly "increment": (args: CounterLocalIncrementArgs, context?: ContextHandle) => Promise<CounterLocalIncrementResult>;
  readonly "read": (context?: ContextHandle) => Promise<CounterLocalReadResult>;
}

export type CounterReference = CounterLocalReference;

export const Counter = fromDescriptor(parsePortableJson("{\"descriptorVersion\":1,\"fingerprint\":\"d14883454c7ebdf65c1baef1429b60ca7a6637902c754e322944ad77ca775bb2\",\"input\":{\"title\":\"CounterInput\",\"type\":\"object\"},\"kind\":\"local\",\"name\":\"swarmie.counter\",\"variants\":{\"\":{\"count\":{\"kind\":\"state\",\"schema\":{\"maximum\":4294967295,\"minimum\":0,\"title\":\"uint32\",\"type\":\"integer\"}},\"increment\":{\"args\":{\"maximum\":4294967295,\"minimum\":0,\"title\":\"uint32\",\"type\":\"integer\"},\"kind\":\"method\",\"result\":{\"maximum\":4294967295,\"minimum\":0,\"title\":\"uint32\",\"type\":\"integer\"}},\"read\":{\"kind\":\"method\",\"result\":{\"maximum\":4294967295,\"minimum\":0,\"title\":\"uint32\",\"type\":\"integer\"}}}},\"version\":1}")) as GeneratedContract<CounterInput, CounterReference>;

export function bindCounter(port: BindingPort): CounterReference {
  return bindReference(Counter, port);
}

export type CounterAcquisitionInput = CounterInput;
