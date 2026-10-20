// Generated from a verified Tembra descriptor. Do not edit.

import { bindReference, fromDescriptor, parsePortableJson, type BindingPort, type ContextHandle, type ContractReference, type GeneratedContract, type Readable } from "@tembra/sdk";

export interface RemoteCounterInput {
  [k: string]: unknown | undefined;
}


export type RemoteCounterMainCountState = number;


export type RemoteCounterMainIncrementArgs = number;


export type RemoteCounterMainIncrementResult = number;


export type RemoteCounterMainReadResult = number;


export interface RemoteCounterMainReference extends ContractReference {
  readonly variant: "main" | undefined;
  readonly "count": Readable<RemoteCounterMainCountState>;
  readonly "increment": (args: RemoteCounterMainIncrementArgs, context?: ContextHandle) => Promise<RemoteCounterMainIncrementResult>;
  readonly "read": (context?: ContextHandle) => Promise<RemoteCounterMainReadResult>;
}

export type RemoteCounterReference = RemoteCounterMainReference;

export const RemoteCounter = fromDescriptor(parsePortableJson("{\"descriptorVersion\":1,\"fingerprint\":\"f9639ef51a437d956a67a4b8fc3309ccdafd667e435faa07292474d08f69ebc1\",\"input\":{\"title\":\"RemoteCounterInput\",\"type\":\"object\"},\"kind\":\"remote\",\"name\":\"swarmie.remote-counter\",\"placement\":{\"name\":\"swarmie-runtime\"},\"variants\":{\"main\":{\"count\":{\"kind\":\"state\",\"schema\":{\"maximum\":4294967295,\"minimum\":0,\"title\":\"uint32\",\"type\":\"integer\"}},\"increment\":{\"args\":{\"maximum\":4294967295,\"minimum\":0,\"title\":\"uint32\",\"type\":\"integer\"},\"kind\":\"method\",\"result\":{\"maximum\":4294967295,\"minimum\":0,\"title\":\"uint32\",\"type\":\"integer\"}},\"read\":{\"kind\":\"method\",\"result\":{\"maximum\":4294967295,\"minimum\":0,\"title\":\"uint32\",\"type\":\"integer\"}}}},\"version\":1}")) as GeneratedContract<RemoteCounterInput, RemoteCounterReference>;

export function bindRemoteCounter(port: BindingPort): RemoteCounterReference {
  return bindReference(RemoteCounter, port);
}

export type RemoteCounterAcquisitionInput = RemoteCounterInput;
