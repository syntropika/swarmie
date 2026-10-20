// Generated from a verified Tembra descriptor. Do not edit.

import { bindReference, fromDescriptor, parsePortableJson, type BindingPort, type ContextHandle, type ContractReference, type GeneratedContract, type Readable } from "@tembra/sdk";

export interface DynamicConsumerV2Input {
  [k: string]: unknown | undefined;
}


export type DynamicConsumerV2LocalInvocationsState = number;


export type DynamicConsumerV2LocalRunResult = string;


export interface DynamicConsumerV2LocalReference extends ContractReference {
  readonly variant: undefined;
  readonly "invocations": Readable<DynamicConsumerV2LocalInvocationsState>;
  readonly "run": (context?: ContextHandle) => Promise<DynamicConsumerV2LocalRunResult>;
}

export type DynamicConsumerV2Reference = DynamicConsumerV2LocalReference;

export const DynamicConsumerV2 = fromDescriptor(parsePortableJson("{\"descriptorVersion\":1,\"fingerprint\":\"f55a961f003a87d30f06e1060e6432b912b7c923a31a6061092f74ad99070a44\",\"input\":{\"title\":\"IncompatibleDynamicConsumerInput\",\"type\":\"object\"},\"kind\":\"local\",\"name\":\"swarmie.dynamic-consumer\",\"variants\":{\"\":{\"invocations\":{\"kind\":\"state\",\"schema\":{\"maximum\":4294967295,\"minimum\":0,\"title\":\"uint32\",\"type\":\"integer\"}},\"run\":{\"kind\":\"method\",\"result\":{\"title\":\"string\",\"type\":\"string\"}}}},\"version\":2}")) as GeneratedContract<DynamicConsumerV2Input, DynamicConsumerV2Reference>;

export function bindDynamicConsumerV2(port: BindingPort): DynamicConsumerV2Reference {
  return bindReference(DynamicConsumerV2, port);
}

export type DynamicConsumerV2AcquisitionInput = DynamicConsumerV2Input;
