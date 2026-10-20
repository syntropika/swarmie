// Generated from a verified Tembra descriptor. Do not edit.

import { bindReference, fromDescriptor, parsePortableJson, type BindingPort, type ContextHandle, type ContractReference, type GeneratedContract, type Readable } from "@tembra/sdk";

export interface DynamicConsumerInput {
  [k: string]: unknown | undefined;
}


export type DynamicConsumerLocalInvocationsState = number;


export type DynamicConsumerLocalRunResult = number;


export interface DynamicConsumerLocalReference extends ContractReference {
  readonly variant: undefined;
  readonly "invocations": Readable<DynamicConsumerLocalInvocationsState>;
  readonly "run": (context?: ContextHandle) => Promise<DynamicConsumerLocalRunResult>;
}

export type DynamicConsumerReference = DynamicConsumerLocalReference;

export const DynamicConsumer = fromDescriptor(parsePortableJson("{\"descriptorVersion\":1,\"fingerprint\":\"f0808f6bd38e98d402d13211c1f160b6a689ba50c0361a38b122255f6d0c1257\",\"input\":{\"title\":\"DynamicConsumerInput\",\"type\":\"object\"},\"kind\":\"local\",\"name\":\"swarmie.dynamic-consumer\",\"variants\":{\"\":{\"invocations\":{\"kind\":\"state\",\"schema\":{\"maximum\":4294967295,\"minimum\":0,\"title\":\"uint32\",\"type\":\"integer\"}},\"run\":{\"kind\":\"method\",\"result\":{\"maximum\":4294967295,\"minimum\":0,\"title\":\"uint32\",\"type\":\"integer\"}}}},\"version\":1}")) as GeneratedContract<DynamicConsumerInput, DynamicConsumerReference>;

export function bindDynamicConsumer(port: BindingPort): DynamicConsumerReference {
  return bindReference(DynamicConsumer, port);
}

export type DynamicConsumerAcquisitionInput = DynamicConsumerInput;
