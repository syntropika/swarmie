// Generated from a verified Tembra descriptor. Do not edit.

import { bindReference, fromDescriptor, parsePortableJson, type BindingPort, type ContextHandle, type ContractReference, type GeneratedContract, type Readable } from "@tembra/sdk";

/**
 * Selects immutable effective application configuration for one configured plugin package.
 */
export interface PluginConfigurationInput {
  plugin_id: string;
}


export interface PluginConfigurationLocalReadResult {
  [k: string]: unknown | undefined;
}


export interface PluginConfigurationLocalReference extends ContractReference {
  readonly variant: undefined;
  readonly "read": (context?: ContextHandle) => Promise<PluginConfigurationLocalReadResult>;
}

export type PluginConfigurationReference = PluginConfigurationLocalReference;

export const PluginConfiguration = fromDescriptor(parsePortableJson("{\"descriptorVersion\":1,\"fingerprint\":\"1dbeb45262ae0c873399f3f8d9bde4d1701d73c4f1d1404b253e5220f1b65911\",\"input\":{\"additionalProperties\":false,\"description\":\"Selects immutable effective application configuration for one configured plugin package.\",\"properties\":{\"plugin_id\":{\"type\":\"string\"}},\"required\":[\"plugin_id\"],\"title\":\"PluginConfigurationInput\",\"type\":\"object\"},\"kind\":\"local\",\"name\":\"swarmie.plugin-configuration\",\"variants\":{\"\":{\"read\":{\"kind\":\"method\",\"result\":{\"title\":\"AnyValue\"}}}},\"version\":1}")) as GeneratedContract<PluginConfigurationInput, PluginConfigurationReference>;

export function bindPluginConfiguration(port: BindingPort): PluginConfigurationReference {
  return bindReference(PluginConfiguration, port);
}

export type PluginConfigurationAcquisitionInput = PluginConfigurationInput;
