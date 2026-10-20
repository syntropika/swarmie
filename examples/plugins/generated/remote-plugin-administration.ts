// Generated from a verified Tembra descriptor. Do not edit.

import { bindReference, fromDescriptor, parsePortableJson, type BindingPort, type ContextHandle, type ContractReference, type GeneratedContract, type Readable } from "@tembra/sdk";

export interface RemotePluginAdministrationInput {
  [k: string]: unknown | undefined;
}


/**
 * An installed plugin selected for activation.
 */
export interface RemotePluginAdministrationMainActivateArgs {
  plugin_id: string;
  /**
   * Opaque administrative capability material, verified by the receiving supervisor.
   *
   * Construction only wraps a value. It does not authenticate a caller or grant permission.
   * Debug output is redacted; serialization intentionally carries the proof to its verifier.
   */
  proof: string;
}


/**
 * Sanitized administrative snapshot rebuilt from durable records and live discovery.
 */
export interface RemotePluginAdministrationMainActivateResult {
  /**
   * Items: Persisted browser session evidence; an empty error denotes no reported error.
   */
  browser_sessions: {
    activation_id: string;
    error: string;
    /**
     * Browser-local execution observation, distinct from server plugin availability.
     * Unknown means the supervisor has no current confirmation of the session's execution.
     */
    observed: "active" | "inactive" | "failed" | "unknown";
    plugin_id: string;
    session_id: string;
    version: string;
  }[];
  /**
   * Items: Durable operation evidence; diagnostics can retain lingering physical work after logical success.
   */
  operations: {
    error: string;
    id: string;
    /**
     * Administrative operation categories persisted for reconciliation.
     */
    operation: "install" | "activate" | "deactivate" | "update" | "uninstall" | "configure_provider" | "recover";
    plugin_id: string;
    /**
     * Administrative progress, distinct from plugin availability.
     */
    status: "pending" | "running" | "succeeded" | "failed" | "interrupted";
  }[];
  /**
   * Items: Installed release and its independently tracked intent and observation.
   */
  plugins: {
    /**
     * Durable administrative intent.
     */
    desired: "installed" | "active" | "absent";
    errors: string[];
    /**
     * How a plugin enters the runtime.
     */
    kind: "core" | "dynamic";
    /**
     * Sanitized server and administrative availability on the connected node.
     * Browser execution is reported separately for each browser session. Inactive concerns
     * logical admission; lingering physical execution remains explicit in operation diagnostics.
     */
    observed: "blocked" | "installed" | "active" | "inactive" | "failed" | "unavailable";
    plugin_id: string;
    provider_id: string;
    version: string;
  }[];
  /**
   * Items: A discovered source provider and its administrative settings.
   */
  providers: {
    configuration: unknown;
    /**
     * Items: Source-provider operation names used in registry metadata and checked schemas.
     */
    operations: ("search" | "resolve" | "inspect" | "acquire")[];
    provider_id: string;
  }[];
}


/**
 * An authenticated request for the currently admitted browser package.
 */
export interface RemotePluginAdministrationMainActivePackageArgs {
  plugin_id: string;
  /**
   * Opaque administrative capability material, verified by the receiving supervisor.
   *
   * Construction only wraps a value. It does not authenticate a caller or grant permission.
   * Debug output is redacted; serialization intentionally carries the proof to its verifier.
   */
  proof: string;
}


/**
 * Approved package metadata and the activation receipt required for bounded file reads.
 */
export interface RemotePluginAdministrationMainActivePackageResult {
  activation_id: string;
  /**
   * The plugin metadata consumed by administration and generated browser bindings.
   */
  manifest: {
    /**
     * Runtime and dependency requirements, checked before admitting plugin artifacts.
     */
    compatibility: {
      swarmie: string;
      tembra_revision: string;
    };
    /**
     * Items: A required plugin version range.
     */
    dependencies: {
      id: string;
      version: string;
    }[];
    /**
     * Human-readable distribution metadata; this does not select a manager implementation.
     */
    distribution: {
      description: string;
      provider: string;
    };
    /**
     * Items: A verified Tembra artifact and its selected module export.
     */
    facets: {
      artifact_path: string;
      /**
       * Supported Tembra loading mechanisms; native browser ESM is not embedded server code.
       */
      format?: "bundled" | "native_esm";
      id: string;
      selected_export: string;
      sha256: string;
      /**
       * Supported JavaScript execution locations.
       */
      target: "server" | "browser";
    }[];
    /**
     * Items: An integrity-pinned file relative to the acquired package root.
     */
    files: {
      path: string;
      sha256: string;
    }[];
    id: string;
    schema_version: number;
    /**
     * Items: A lifecycle-owned browser module export, resolved by identity rather than serialized code.
     */
    ui: {
      export_name: string;
      id: string;
      /**
       * Supported browser contribution categories.
       */
      kind: "slot" | "widget" | "command" | "panel" | "screen" | "renderer" | "configuration";
      label: string;
      module_path: string;
      order: number;
      plugin_id: string;
      slot_id: string;
    }[];
    version: string;
  };
  /**
   * Provider-issued acquisition evidence, retained with durable administrative records.
   */
  provenance: {
    integrity: string;
    provider_id: string;
    source: string;
  };
}


/**
 * Durable source-provider settings; interpretation belongs to the provider.
 */
export interface RemotePluginAdministrationMainConfigureProviderArgs {
  configuration: unknown;
  /**
   * Opaque administrative capability material, verified by the receiving supervisor.
   *
   * Construction only wraps a value. It does not authenticate a caller or grant permission.
   * Debug output is redacted; serialization intentionally carries the proof to its verifier.
   */
  proof: string;
  provider_id: string;
}


/**
 * Sanitized administrative snapshot rebuilt from durable records and live discovery.
 */
export interface RemotePluginAdministrationMainConfigureProviderResult {
  /**
   * Items: Persisted browser session evidence; an empty error denotes no reported error.
   */
  browser_sessions: {
    activation_id: string;
    error: string;
    /**
     * Browser-local execution observation, distinct from server plugin availability.
     * Unknown means the supervisor has no current confirmation of the session's execution.
     */
    observed: "active" | "inactive" | "failed" | "unknown";
    plugin_id: string;
    session_id: string;
    version: string;
  }[];
  /**
   * Items: Durable operation evidence; diagnostics can retain lingering physical work after logical success.
   */
  operations: {
    error: string;
    id: string;
    /**
     * Administrative operation categories persisted for reconciliation.
     */
    operation: "install" | "activate" | "deactivate" | "update" | "uninstall" | "configure_provider" | "recover";
    plugin_id: string;
    /**
     * Administrative progress, distinct from plugin availability.
     */
    status: "pending" | "running" | "succeeded" | "failed" | "interrupted";
  }[];
  /**
   * Items: Installed release and its independently tracked intent and observation.
   */
  plugins: {
    /**
     * Durable administrative intent.
     */
    desired: "installed" | "active" | "absent";
    errors: string[];
    /**
     * How a plugin enters the runtime.
     */
    kind: "core" | "dynamic";
    /**
     * Sanitized server and administrative availability on the connected node.
     * Browser execution is reported separately for each browser session. Inactive concerns
     * logical admission; lingering physical execution remains explicit in operation diagnostics.
     */
    observed: "blocked" | "installed" | "active" | "inactive" | "failed" | "unavailable";
    plugin_id: string;
    provider_id: string;
    version: string;
  }[];
  /**
   * Items: A discovered source provider and its administrative settings.
   */
  providers: {
    configuration: unknown;
    /**
     * Items: Source-provider operation names used in registry metadata and checked schemas.
     */
    operations: ("search" | "resolve" | "inspect" | "acquire")[];
    provider_id: string;
  }[];
}


/**
 * An installed plugin selected for deactivation.
 */
export interface RemotePluginAdministrationMainDeactivateArgs {
  plugin_id: string;
  /**
   * Opaque administrative capability material, verified by the receiving supervisor.
   *
   * Construction only wraps a value. It does not authenticate a caller or grant permission.
   * Debug output is redacted; serialization intentionally carries the proof to its verifier.
   */
  proof: string;
}


/**
 * Sanitized administrative snapshot rebuilt from durable records and live discovery.
 */
export interface RemotePluginAdministrationMainDeactivateResult {
  /**
   * Items: Persisted browser session evidence; an empty error denotes no reported error.
   */
  browser_sessions: {
    activation_id: string;
    error: string;
    /**
     * Browser-local execution observation, distinct from server plugin availability.
     * Unknown means the supervisor has no current confirmation of the session's execution.
     */
    observed: "active" | "inactive" | "failed" | "unknown";
    plugin_id: string;
    session_id: string;
    version: string;
  }[];
  /**
   * Items: Durable operation evidence; diagnostics can retain lingering physical work after logical success.
   */
  operations: {
    error: string;
    id: string;
    /**
     * Administrative operation categories persisted for reconciliation.
     */
    operation: "install" | "activate" | "deactivate" | "update" | "uninstall" | "configure_provider" | "recover";
    plugin_id: string;
    /**
     * Administrative progress, distinct from plugin availability.
     */
    status: "pending" | "running" | "succeeded" | "failed" | "interrupted";
  }[];
  /**
   * Items: Installed release and its independently tracked intent and observation.
   */
  plugins: {
    /**
     * Durable administrative intent.
     */
    desired: "installed" | "active" | "absent";
    errors: string[];
    /**
     * How a plugin enters the runtime.
     */
    kind: "core" | "dynamic";
    /**
     * Sanitized server and administrative availability on the connected node.
     * Browser execution is reported separately for each browser session. Inactive concerns
     * logical admission; lingering physical execution remains explicit in operation diagnostics.
     */
    observed: "blocked" | "installed" | "active" | "inactive" | "failed" | "unavailable";
    plugin_id: string;
    provider_id: string;
    version: string;
  }[];
  /**
   * Items: A discovered source provider and its administrative settings.
   */
  providers: {
    configuration: unknown;
    /**
     * Items: Source-provider operation names used in registry metadata and checked schemas.
     */
    operations: ("search" | "resolve" | "inspect" | "acquire")[];
    provider_id: string;
  }[];
}


/**
 * An explicitly confirmed installation candidate.
 */
export interface RemotePluginAdministrationMainInstallArgs {
  confirmed: boolean;
  plugin_id: string;
  /**
   * Opaque administrative capability material, verified by the receiving supervisor.
   *
   * Construction only wraps a value. It does not authenticate a caller or grant permission.
   * Debug output is redacted; serialization intentionally carries the proof to its verifier.
   */
  proof: string;
  provider_id: string;
  version: string;
}


/**
 * Sanitized administrative snapshot rebuilt from durable records and live discovery.
 */
export interface RemotePluginAdministrationMainInstallResult {
  /**
   * Items: Persisted browser session evidence; an empty error denotes no reported error.
   */
  browser_sessions: {
    activation_id: string;
    error: string;
    /**
     * Browser-local execution observation, distinct from server plugin availability.
     * Unknown means the supervisor has no current confirmation of the session's execution.
     */
    observed: "active" | "inactive" | "failed" | "unknown";
    plugin_id: string;
    session_id: string;
    version: string;
  }[];
  /**
   * Items: Durable operation evidence; diagnostics can retain lingering physical work after logical success.
   */
  operations: {
    error: string;
    id: string;
    /**
     * Administrative operation categories persisted for reconciliation.
     */
    operation: "install" | "activate" | "deactivate" | "update" | "uninstall" | "configure_provider" | "recover";
    plugin_id: string;
    /**
     * Administrative progress, distinct from plugin availability.
     */
    status: "pending" | "running" | "succeeded" | "failed" | "interrupted";
  }[];
  /**
   * Items: Installed release and its independently tracked intent and observation.
   */
  plugins: {
    /**
     * Durable administrative intent.
     */
    desired: "installed" | "active" | "absent";
    errors: string[];
    /**
     * How a plugin enters the runtime.
     */
    kind: "core" | "dynamic";
    /**
     * Sanitized server and administrative availability on the connected node.
     * Browser execution is reported separately for each browser session. Inactive concerns
     * logical admission; lingering physical execution remains explicit in operation diagnostics.
     */
    observed: "blocked" | "installed" | "active" | "inactive" | "failed" | "unavailable";
    plugin_id: string;
    provider_id: string;
    version: string;
  }[];
  /**
   * Items: A discovered source provider and its administrative settings.
   */
  providers: {
    configuration: unknown;
    /**
     * Items: Source-provider operation names used in registry metadata and checked schemas.
     */
    operations: ("search" | "resolve" | "inspect" | "acquire")[];
    provider_id: string;
  }[];
}


/**
 * An authenticated inventory query.
 */
export interface RemotePluginAdministrationMainInventoryArgs {
  /**
   * Opaque administrative capability material, verified by the receiving supervisor.
   *
   * Construction only wraps a value. It does not authenticate a caller or grant permission.
   * Debug output is redacted; serialization intentionally carries the proof to its verifier.
   */
  proof: string;
}


/**
 * Sanitized administrative snapshot rebuilt from durable records and live discovery.
 */
export interface RemotePluginAdministrationMainInventoryResult {
  /**
   * Items: Persisted browser session evidence; an empty error denotes no reported error.
   */
  browser_sessions: {
    activation_id: string;
    error: string;
    /**
     * Browser-local execution observation, distinct from server plugin availability.
     * Unknown means the supervisor has no current confirmation of the session's execution.
     */
    observed: "active" | "inactive" | "failed" | "unknown";
    plugin_id: string;
    session_id: string;
    version: string;
  }[];
  /**
   * Items: Durable operation evidence; diagnostics can retain lingering physical work after logical success.
   */
  operations: {
    error: string;
    id: string;
    /**
     * Administrative operation categories persisted for reconciliation.
     */
    operation: "install" | "activate" | "deactivate" | "update" | "uninstall" | "configure_provider" | "recover";
    plugin_id: string;
    /**
     * Administrative progress, distinct from plugin availability.
     */
    status: "pending" | "running" | "succeeded" | "failed" | "interrupted";
  }[];
  /**
   * Items: Installed release and its independently tracked intent and observation.
   */
  plugins: {
    /**
     * Durable administrative intent.
     */
    desired: "installed" | "active" | "absent";
    errors: string[];
    /**
     * How a plugin enters the runtime.
     */
    kind: "core" | "dynamic";
    /**
     * Sanitized server and administrative availability on the connected node.
     * Browser execution is reported separately for each browser session. Inactive concerns
     * logical admission; lingering physical execution remains explicit in operation diagnostics.
     */
    observed: "blocked" | "installed" | "active" | "inactive" | "failed" | "unavailable";
    plugin_id: string;
    provider_id: string;
    version: string;
  }[];
  /**
   * Items: A discovered source provider and its administrative settings.
   */
  providers: {
    configuration: unknown;
    /**
     * Items: Source-provider operation names used in registry metadata and checked schemas.
     */
    operations: ("search" | "resolve" | "inspect" | "acquire")[];
    provider_id: string;
  }[];
}


/**
 * A receipt-bound request for at most 32 KiB of an approved package file.
 */
export interface RemotePluginAdministrationMainReadPackageFileArgs {
  activation_id: string;
  length: number;
  offset: number;
  path: string;
  plugin_id: string;
  /**
   * Opaque administrative capability material, verified by the receiving supervisor.
   *
   * Construction only wraps a value. It does not authenticate a caller or grant permission.
   * Debug output is redacted; serialization intentionally carries the proof to its verifier.
   */
  proof: string;
}


/**
 * A bounded exact byte range and the stable approved file length.
 */
export interface RemotePluginAdministrationMainReadPackageFileResult {
  /**
   * @maxItems 32768
   */
  bytes: number[];
  total_length: number;
}


/**
 * An authenticated browser acknowledgement bound to a specific activation and session.
 */
export interface RemotePluginAdministrationMainReportBrowserArgs {
  activation_id: string;
  error: string;
  /**
   * Browser-local execution observation, distinct from server plugin availability.
   * Unknown means the supervisor has no current confirmation of the session's execution.
   */
  observed: "active" | "inactive" | "failed" | "unknown";
  plugin_id: string;
  /**
   * Opaque administrative capability material, verified by the receiving supervisor.
   *
   * Construction only wraps a value. It does not authenticate a caller or grant permission.
   * Debug output is redacted; serialization intentionally carries the proof to its verifier.
   */
  proof: string;
  session_id: string;
  version: string;
}


/**
 * Sanitized administrative snapshot rebuilt from durable records and live discovery.
 */
export interface RemotePluginAdministrationMainReportBrowserResult {
  /**
   * Items: Persisted browser session evidence; an empty error denotes no reported error.
   */
  browser_sessions: {
    activation_id: string;
    error: string;
    /**
     * Browser-local execution observation, distinct from server plugin availability.
     * Unknown means the supervisor has no current confirmation of the session's execution.
     */
    observed: "active" | "inactive" | "failed" | "unknown";
    plugin_id: string;
    session_id: string;
    version: string;
  }[];
  /**
   * Items: Durable operation evidence; diagnostics can retain lingering physical work after logical success.
   */
  operations: {
    error: string;
    id: string;
    /**
     * Administrative operation categories persisted for reconciliation.
     */
    operation: "install" | "activate" | "deactivate" | "update" | "uninstall" | "configure_provider" | "recover";
    plugin_id: string;
    /**
     * Administrative progress, distinct from plugin availability.
     */
    status: "pending" | "running" | "succeeded" | "failed" | "interrupted";
  }[];
  /**
   * Items: Installed release and its independently tracked intent and observation.
   */
  plugins: {
    /**
     * Durable administrative intent.
     */
    desired: "installed" | "active" | "absent";
    errors: string[];
    /**
     * How a plugin enters the runtime.
     */
    kind: "core" | "dynamic";
    /**
     * Sanitized server and administrative availability on the connected node.
     * Browser execution is reported separately for each browser session. Inactive concerns
     * logical admission; lingering physical execution remains explicit in operation diagnostics.
     */
    observed: "blocked" | "installed" | "active" | "inactive" | "failed" | "unavailable";
    plugin_id: string;
    provider_id: string;
    version: string;
  }[];
  /**
   * Items: A discovered source provider and its administrative settings.
   */
  providers: {
    configuration: unknown;
    /**
     * Items: Source-provider operation names used in registry metadata and checked schemas.
     */
    operations: ("search" | "resolve" | "inspect" | "acquire")[];
    provider_id: string;
  }[];
}


/**
 * An installed plugin selected for removal.
 */
export interface RemotePluginAdministrationMainUninstallArgs {
  plugin_id: string;
  /**
   * Opaque administrative capability material, verified by the receiving supervisor.
   *
   * Construction only wraps a value. It does not authenticate a caller or grant permission.
   * Debug output is redacted; serialization intentionally carries the proof to its verifier.
   */
  proof: string;
}


/**
 * Sanitized administrative snapshot rebuilt from durable records and live discovery.
 */
export interface RemotePluginAdministrationMainUninstallResult {
  /**
   * Items: Persisted browser session evidence; an empty error denotes no reported error.
   */
  browser_sessions: {
    activation_id: string;
    error: string;
    /**
     * Browser-local execution observation, distinct from server plugin availability.
     * Unknown means the supervisor has no current confirmation of the session's execution.
     */
    observed: "active" | "inactive" | "failed" | "unknown";
    plugin_id: string;
    session_id: string;
    version: string;
  }[];
  /**
   * Items: Durable operation evidence; diagnostics can retain lingering physical work after logical success.
   */
  operations: {
    error: string;
    id: string;
    /**
     * Administrative operation categories persisted for reconciliation.
     */
    operation: "install" | "activate" | "deactivate" | "update" | "uninstall" | "configure_provider" | "recover";
    plugin_id: string;
    /**
     * Administrative progress, distinct from plugin availability.
     */
    status: "pending" | "running" | "succeeded" | "failed" | "interrupted";
  }[];
  /**
   * Items: Installed release and its independently tracked intent and observation.
   */
  plugins: {
    /**
     * Durable administrative intent.
     */
    desired: "installed" | "active" | "absent";
    errors: string[];
    /**
     * How a plugin enters the runtime.
     */
    kind: "core" | "dynamic";
    /**
     * Sanitized server and administrative availability on the connected node.
     * Browser execution is reported separately for each browser session. Inactive concerns
     * logical admission; lingering physical execution remains explicit in operation diagnostics.
     */
    observed: "blocked" | "installed" | "active" | "inactive" | "failed" | "unavailable";
    plugin_id: string;
    provider_id: string;
    version: string;
  }[];
  /**
   * Items: A discovered source provider and its administrative settings.
   */
  providers: {
    configuration: unknown;
    /**
     * Items: Source-provider operation names used in registry metadata and checked schemas.
     */
    operations: ("search" | "resolve" | "inspect" | "acquire")[];
    provider_id: string;
  }[];
}


/**
 * An explicitly confirmed replacement for an installed plugin.
 */
export interface RemotePluginAdministrationMainUpdateArgs {
  confirmed: boolean;
  plugin_id: string;
  /**
   * Opaque administrative capability material, verified by the receiving supervisor.
   *
   * Construction only wraps a value. It does not authenticate a caller or grant permission.
   * Debug output is redacted; serialization intentionally carries the proof to its verifier.
   */
  proof: string;
  provider_id: string;
  version: string;
}


/**
 * Sanitized administrative snapshot rebuilt from durable records and live discovery.
 */
export interface RemotePluginAdministrationMainUpdateResult {
  /**
   * Items: Persisted browser session evidence; an empty error denotes no reported error.
   */
  browser_sessions: {
    activation_id: string;
    error: string;
    /**
     * Browser-local execution observation, distinct from server plugin availability.
     * Unknown means the supervisor has no current confirmation of the session's execution.
     */
    observed: "active" | "inactive" | "failed" | "unknown";
    plugin_id: string;
    session_id: string;
    version: string;
  }[];
  /**
   * Items: Durable operation evidence; diagnostics can retain lingering physical work after logical success.
   */
  operations: {
    error: string;
    id: string;
    /**
     * Administrative operation categories persisted for reconciliation.
     */
    operation: "install" | "activate" | "deactivate" | "update" | "uninstall" | "configure_provider" | "recover";
    plugin_id: string;
    /**
     * Administrative progress, distinct from plugin availability.
     */
    status: "pending" | "running" | "succeeded" | "failed" | "interrupted";
  }[];
  /**
   * Items: Installed release and its independently tracked intent and observation.
   */
  plugins: {
    /**
     * Durable administrative intent.
     */
    desired: "installed" | "active" | "absent";
    errors: string[];
    /**
     * How a plugin enters the runtime.
     */
    kind: "core" | "dynamic";
    /**
     * Sanitized server and administrative availability on the connected node.
     * Browser execution is reported separately for each browser session. Inactive concerns
     * logical admission; lingering physical execution remains explicit in operation diagnostics.
     */
    observed: "blocked" | "installed" | "active" | "inactive" | "failed" | "unavailable";
    plugin_id: string;
    provider_id: string;
    version: string;
  }[];
  /**
   * Items: A discovered source provider and its administrative settings.
   */
  providers: {
    configuration: unknown;
    /**
     * Items: Source-provider operation names used in registry metadata and checked schemas.
     */
    operations: ("search" | "resolve" | "inspect" | "acquire")[];
    provider_id: string;
  }[];
}


export interface RemotePluginAdministrationMainReference extends ContractReference {
  readonly variant: "main" | undefined;
  readonly "activate": (args: RemotePluginAdministrationMainActivateArgs, context?: ContextHandle) => Promise<RemotePluginAdministrationMainActivateResult>;
  readonly "active_package": (args: RemotePluginAdministrationMainActivePackageArgs, context?: ContextHandle) => Promise<RemotePluginAdministrationMainActivePackageResult>;
  readonly "configure_provider": (args: RemotePluginAdministrationMainConfigureProviderArgs, context?: ContextHandle) => Promise<RemotePluginAdministrationMainConfigureProviderResult>;
  readonly "deactivate": (args: RemotePluginAdministrationMainDeactivateArgs, context?: ContextHandle) => Promise<RemotePluginAdministrationMainDeactivateResult>;
  readonly "install": (args: RemotePluginAdministrationMainInstallArgs, context?: ContextHandle) => Promise<RemotePluginAdministrationMainInstallResult>;
  readonly "inventory": (args: RemotePluginAdministrationMainInventoryArgs, context?: ContextHandle) => Promise<RemotePluginAdministrationMainInventoryResult>;
  readonly "read_package_file": (args: RemotePluginAdministrationMainReadPackageFileArgs, context?: ContextHandle) => Promise<RemotePluginAdministrationMainReadPackageFileResult>;
  readonly "report_browser": (args: RemotePluginAdministrationMainReportBrowserArgs, context?: ContextHandle) => Promise<RemotePluginAdministrationMainReportBrowserResult>;
  readonly "uninstall": (args: RemotePluginAdministrationMainUninstallArgs, context?: ContextHandle) => Promise<RemotePluginAdministrationMainUninstallResult>;
  readonly "update": (args: RemotePluginAdministrationMainUpdateArgs, context?: ContextHandle) => Promise<RemotePluginAdministrationMainUpdateResult>;
}

export type RemotePluginAdministrationReference = RemotePluginAdministrationMainReference;

export const RemotePluginAdministration = fromDescriptor(parsePortableJson("{\"descriptorVersion\":1,\"fingerprint\":\"d4c2648502177f7326c144306a165e600e27f0af263e54dbe90a73054e93dc72\",\"input\":{\"title\":\"RemotePluginAdministrationInput\",\"type\":\"object\"},\"kind\":\"remote\",\"name\":\"swarmie.remote-plugin-administration\",\"placement\":{\"name\":\"swarmie-runtime\"},\"variants\":{\"main\":{\"activate\":{\"args\":{\"additionalProperties\":false,\"description\":\"An installed plugin selected for activation.\",\"properties\":{\"plugin_id\":{\"type\":\"string\"},\"proof\":{\"description\":\"Opaque administrative capability material, verified by the receiving supervisor.\\n\\nConstruction only wraps a value. It does not authenticate a caller or grant permission.\\nDebug output is redacted; serialization intentionally carries the proof to its verifier.\",\"type\":\"string\"}},\"required\":[\"proof\",\"plugin_id\"],\"title\":\"ActivateRequest\",\"type\":\"object\"},\"kind\":\"method\",\"result\":{\"additionalProperties\":false,\"description\":\"Sanitized administrative snapshot rebuilt from durable records and live discovery.\",\"properties\":{\"browser_sessions\":{\"items\":{\"additionalProperties\":false,\"description\":\"Persisted browser session evidence; an empty error denotes no reported error.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"error\":{\"type\":\"string\"},\"observed\":{\"description\":\"Browser-local execution observation, distinct from server plugin availability.\\nUnknown means the supervisor has no current confirmation of the session's execution.\",\"enum\":[\"active\",\"inactive\",\"failed\",\"unknown\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"session_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"activation_id\",\"session_id\",\"observed\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"operations\":{\"items\":{\"additionalProperties\":false,\"description\":\"Durable operation evidence; diagnostics can retain lingering physical work after logical success.\",\"properties\":{\"error\":{\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"operation\":{\"description\":\"Administrative operation categories persisted for reconciliation.\",\"enum\":[\"install\",\"activate\",\"deactivate\",\"update\",\"uninstall\",\"configure_provider\",\"recover\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"status\":{\"description\":\"Administrative progress, distinct from plugin availability.\",\"enum\":[\"pending\",\"running\",\"succeeded\",\"failed\",\"interrupted\"],\"type\":\"string\"}},\"required\":[\"id\",\"plugin_id\",\"operation\",\"status\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"plugins\":{\"items\":{\"additionalProperties\":false,\"description\":\"Installed release and its independently tracked intent and observation.\",\"properties\":{\"desired\":{\"description\":\"Durable administrative intent.\",\"enum\":[\"installed\",\"active\",\"absent\"],\"type\":\"string\"},\"errors\":{\"items\":{\"type\":\"string\"},\"type\":\"array\"},\"kind\":{\"description\":\"How a plugin enters the runtime.\",\"enum\":[\"core\",\"dynamic\"],\"type\":\"string\"},\"observed\":{\"description\":\"Sanitized server and administrative availability on the connected node.\\nBrowser execution is reported separately for each browser session. Inactive concerns\\nlogical admission; lingering physical execution remains explicit in operation diagnostics.\",\"enum\":[\"blocked\",\"installed\",\"active\",\"inactive\",\"failed\",\"unavailable\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"provider_id\",\"kind\",\"desired\",\"observed\",\"errors\"],\"type\":\"object\"},\"type\":\"array\"},\"providers\":{\"items\":{\"additionalProperties\":false,\"description\":\"A discovered source provider and its administrative settings.\",\"properties\":{\"configuration\":{},\"operations\":{\"items\":{\"description\":\"Source-provider operation names used in registry metadata and checked schemas.\",\"enum\":[\"search\",\"resolve\",\"inspect\",\"acquire\"],\"type\":\"string\"},\"type\":\"array\"},\"provider_id\":{\"type\":\"string\"}},\"required\":[\"provider_id\",\"configuration\",\"operations\"],\"type\":\"object\"},\"type\":\"array\"}},\"required\":[\"plugins\",\"providers\",\"operations\",\"browser_sessions\"],\"title\":\"Inventory\",\"type\":\"object\"}},\"active_package\":{\"args\":{\"additionalProperties\":false,\"description\":\"An authenticated request for the currently admitted browser package.\",\"properties\":{\"plugin_id\":{\"type\":\"string\"},\"proof\":{\"description\":\"Opaque administrative capability material, verified by the receiving supervisor.\\n\\nConstruction only wraps a value. It does not authenticate a caller or grant permission.\\nDebug output is redacted; serialization intentionally carries the proof to its verifier.\",\"type\":\"string\"}},\"required\":[\"proof\",\"plugin_id\"],\"title\":\"ActivePackageRequest\",\"type\":\"object\"},\"kind\":\"method\",\"result\":{\"additionalProperties\":false,\"description\":\"Approved package metadata and the activation receipt required for bounded file reads.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"manifest\":{\"additionalProperties\":false,\"description\":\"The plugin metadata consumed by administration and generated browser bindings.\",\"properties\":{\"compatibility\":{\"additionalProperties\":false,\"description\":\"Runtime and dependency requirements, checked before admitting plugin artifacts.\",\"properties\":{\"swarmie\":{\"type\":\"string\"},\"tembra_revision\":{\"type\":\"string\"}},\"required\":[\"swarmie\",\"tembra_revision\"],\"type\":\"object\"},\"dependencies\":{\"items\":{\"additionalProperties\":false,\"description\":\"A required plugin version range.\",\"properties\":{\"id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"id\",\"version\"],\"type\":\"object\"},\"type\":\"array\"},\"distribution\":{\"additionalProperties\":false,\"description\":\"Human-readable distribution metadata; this does not select a manager implementation.\",\"properties\":{\"description\":{\"type\":\"string\"},\"provider\":{\"type\":\"string\"}},\"required\":[\"provider\",\"description\"],\"type\":\"object\"},\"facets\":{\"items\":{\"additionalProperties\":false,\"description\":\"A verified Tembra artifact and its selected module export.\",\"properties\":{\"artifact_path\":{\"type\":\"string\"},\"format\":{\"description\":\"Supported Tembra loading mechanisms; native browser ESM is not embedded server code.\",\"enum\":[\"bundled\",\"native_esm\"],\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"selected_export\":{\"type\":\"string\"},\"sha256\":{\"type\":\"string\"},\"target\":{\"description\":\"Supported JavaScript execution locations.\",\"enum\":[\"server\",\"browser\"],\"type\":\"string\"}},\"required\":[\"id\",\"target\",\"artifact_path\",\"sha256\",\"selected_export\"],\"type\":\"object\"},\"type\":\"array\"},\"files\":{\"items\":{\"additionalProperties\":false,\"description\":\"An integrity-pinned file relative to the acquired package root.\",\"properties\":{\"path\":{\"type\":\"string\"},\"sha256\":{\"type\":\"string\"}},\"required\":[\"path\",\"sha256\"],\"type\":\"object\"},\"type\":\"array\"},\"id\":{\"type\":\"string\"},\"schema_version\":{\"maximum\":4294967295,\"minimum\":0,\"type\":\"integer\"},\"ui\":{\"items\":{\"additionalProperties\":false,\"description\":\"A lifecycle-owned browser module export, resolved by identity rather than serialized code.\",\"properties\":{\"export_name\":{\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"kind\":{\"description\":\"Supported browser contribution categories.\",\"enum\":[\"slot\",\"widget\",\"command\",\"panel\",\"screen\",\"renderer\",\"configuration\"],\"type\":\"string\"},\"label\":{\"type\":\"string\"},\"module_path\":{\"type\":\"string\"},\"order\":{\"maximum\":2147483647,\"minimum\":-2147483648,\"type\":\"integer\"},\"plugin_id\":{\"type\":\"string\"},\"slot_id\":{\"type\":\"string\"}},\"required\":[\"id\",\"plugin_id\",\"kind\",\"label\",\"slot_id\",\"order\",\"module_path\",\"export_name\"],\"type\":\"object\"},\"type\":\"array\"},\"version\":{\"type\":\"string\"}},\"required\":[\"schema_version\",\"id\",\"version\",\"compatibility\",\"dependencies\",\"facets\",\"files\",\"distribution\",\"ui\"],\"type\":\"object\"},\"provenance\":{\"additionalProperties\":false,\"description\":\"Provider-issued acquisition evidence, retained with durable administrative records.\",\"properties\":{\"integrity\":{\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"},\"source\":{\"type\":\"string\"}},\"required\":[\"provider_id\",\"source\",\"integrity\"],\"type\":\"object\"}},\"required\":[\"manifest\",\"provenance\",\"activation_id\"],\"title\":\"ActivePackage\",\"type\":\"object\"}},\"configure_provider\":{\"args\":{\"additionalProperties\":false,\"description\":\"Durable source-provider settings; interpretation belongs to the provider.\",\"properties\":{\"configuration\":{},\"proof\":{\"description\":\"Opaque administrative capability material, verified by the receiving supervisor.\\n\\nConstruction only wraps a value. It does not authenticate a caller or grant permission.\\nDebug output is redacted; serialization intentionally carries the proof to its verifier.\",\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"}},\"required\":[\"proof\",\"provider_id\",\"configuration\"],\"title\":\"ConfigureProviderRequest\",\"type\":\"object\"},\"kind\":\"method\",\"result\":{\"additionalProperties\":false,\"description\":\"Sanitized administrative snapshot rebuilt from durable records and live discovery.\",\"properties\":{\"browser_sessions\":{\"items\":{\"additionalProperties\":false,\"description\":\"Persisted browser session evidence; an empty error denotes no reported error.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"error\":{\"type\":\"string\"},\"observed\":{\"description\":\"Browser-local execution observation, distinct from server plugin availability.\\nUnknown means the supervisor has no current confirmation of the session's execution.\",\"enum\":[\"active\",\"inactive\",\"failed\",\"unknown\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"session_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"activation_id\",\"session_id\",\"observed\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"operations\":{\"items\":{\"additionalProperties\":false,\"description\":\"Durable operation evidence; diagnostics can retain lingering physical work after logical success.\",\"properties\":{\"error\":{\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"operation\":{\"description\":\"Administrative operation categories persisted for reconciliation.\",\"enum\":[\"install\",\"activate\",\"deactivate\",\"update\",\"uninstall\",\"configure_provider\",\"recover\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"status\":{\"description\":\"Administrative progress, distinct from plugin availability.\",\"enum\":[\"pending\",\"running\",\"succeeded\",\"failed\",\"interrupted\"],\"type\":\"string\"}},\"required\":[\"id\",\"plugin_id\",\"operation\",\"status\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"plugins\":{\"items\":{\"additionalProperties\":false,\"description\":\"Installed release and its independently tracked intent and observation.\",\"properties\":{\"desired\":{\"description\":\"Durable administrative intent.\",\"enum\":[\"installed\",\"active\",\"absent\"],\"type\":\"string\"},\"errors\":{\"items\":{\"type\":\"string\"},\"type\":\"array\"},\"kind\":{\"description\":\"How a plugin enters the runtime.\",\"enum\":[\"core\",\"dynamic\"],\"type\":\"string\"},\"observed\":{\"description\":\"Sanitized server and administrative availability on the connected node.\\nBrowser execution is reported separately for each browser session. Inactive concerns\\nlogical admission; lingering physical execution remains explicit in operation diagnostics.\",\"enum\":[\"blocked\",\"installed\",\"active\",\"inactive\",\"failed\",\"unavailable\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"provider_id\",\"kind\",\"desired\",\"observed\",\"errors\"],\"type\":\"object\"},\"type\":\"array\"},\"providers\":{\"items\":{\"additionalProperties\":false,\"description\":\"A discovered source provider and its administrative settings.\",\"properties\":{\"configuration\":{},\"operations\":{\"items\":{\"description\":\"Source-provider operation names used in registry metadata and checked schemas.\",\"enum\":[\"search\",\"resolve\",\"inspect\",\"acquire\"],\"type\":\"string\"},\"type\":\"array\"},\"provider_id\":{\"type\":\"string\"}},\"required\":[\"provider_id\",\"configuration\",\"operations\"],\"type\":\"object\"},\"type\":\"array\"}},\"required\":[\"plugins\",\"providers\",\"operations\",\"browser_sessions\"],\"title\":\"Inventory\",\"type\":\"object\"}},\"deactivate\":{\"args\":{\"additionalProperties\":false,\"description\":\"An installed plugin selected for deactivation.\",\"properties\":{\"plugin_id\":{\"type\":\"string\"},\"proof\":{\"description\":\"Opaque administrative capability material, verified by the receiving supervisor.\\n\\nConstruction only wraps a value. It does not authenticate a caller or grant permission.\\nDebug output is redacted; serialization intentionally carries the proof to its verifier.\",\"type\":\"string\"}},\"required\":[\"proof\",\"plugin_id\"],\"title\":\"DeactivateRequest\",\"type\":\"object\"},\"kind\":\"method\",\"result\":{\"additionalProperties\":false,\"description\":\"Sanitized administrative snapshot rebuilt from durable records and live discovery.\",\"properties\":{\"browser_sessions\":{\"items\":{\"additionalProperties\":false,\"description\":\"Persisted browser session evidence; an empty error denotes no reported error.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"error\":{\"type\":\"string\"},\"observed\":{\"description\":\"Browser-local execution observation, distinct from server plugin availability.\\nUnknown means the supervisor has no current confirmation of the session's execution.\",\"enum\":[\"active\",\"inactive\",\"failed\",\"unknown\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"session_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"activation_id\",\"session_id\",\"observed\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"operations\":{\"items\":{\"additionalProperties\":false,\"description\":\"Durable operation evidence; diagnostics can retain lingering physical work after logical success.\",\"properties\":{\"error\":{\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"operation\":{\"description\":\"Administrative operation categories persisted for reconciliation.\",\"enum\":[\"install\",\"activate\",\"deactivate\",\"update\",\"uninstall\",\"configure_provider\",\"recover\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"status\":{\"description\":\"Administrative progress, distinct from plugin availability.\",\"enum\":[\"pending\",\"running\",\"succeeded\",\"failed\",\"interrupted\"],\"type\":\"string\"}},\"required\":[\"id\",\"plugin_id\",\"operation\",\"status\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"plugins\":{\"items\":{\"additionalProperties\":false,\"description\":\"Installed release and its independently tracked intent and observation.\",\"properties\":{\"desired\":{\"description\":\"Durable administrative intent.\",\"enum\":[\"installed\",\"active\",\"absent\"],\"type\":\"string\"},\"errors\":{\"items\":{\"type\":\"string\"},\"type\":\"array\"},\"kind\":{\"description\":\"How a plugin enters the runtime.\",\"enum\":[\"core\",\"dynamic\"],\"type\":\"string\"},\"observed\":{\"description\":\"Sanitized server and administrative availability on the connected node.\\nBrowser execution is reported separately for each browser session. Inactive concerns\\nlogical admission; lingering physical execution remains explicit in operation diagnostics.\",\"enum\":[\"blocked\",\"installed\",\"active\",\"inactive\",\"failed\",\"unavailable\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"provider_id\",\"kind\",\"desired\",\"observed\",\"errors\"],\"type\":\"object\"},\"type\":\"array\"},\"providers\":{\"items\":{\"additionalProperties\":false,\"description\":\"A discovered source provider and its administrative settings.\",\"properties\":{\"configuration\":{},\"operations\":{\"items\":{\"description\":\"Source-provider operation names used in registry metadata and checked schemas.\",\"enum\":[\"search\",\"resolve\",\"inspect\",\"acquire\"],\"type\":\"string\"},\"type\":\"array\"},\"provider_id\":{\"type\":\"string\"}},\"required\":[\"provider_id\",\"configuration\",\"operations\"],\"type\":\"object\"},\"type\":\"array\"}},\"required\":[\"plugins\",\"providers\",\"operations\",\"browser_sessions\"],\"title\":\"Inventory\",\"type\":\"object\"}},\"install\":{\"args\":{\"additionalProperties\":false,\"description\":\"An explicitly confirmed installation candidate.\",\"properties\":{\"confirmed\":{\"type\":\"boolean\"},\"plugin_id\":{\"type\":\"string\"},\"proof\":{\"description\":\"Opaque administrative capability material, verified by the receiving supervisor.\\n\\nConstruction only wraps a value. It does not authenticate a caller or grant permission.\\nDebug output is redacted; serialization intentionally carries the proof to its verifier.\",\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"proof\",\"provider_id\",\"plugin_id\",\"version\",\"confirmed\"],\"title\":\"InstallRequest\",\"type\":\"object\"},\"kind\":\"method\",\"result\":{\"additionalProperties\":false,\"description\":\"Sanitized administrative snapshot rebuilt from durable records and live discovery.\",\"properties\":{\"browser_sessions\":{\"items\":{\"additionalProperties\":false,\"description\":\"Persisted browser session evidence; an empty error denotes no reported error.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"error\":{\"type\":\"string\"},\"observed\":{\"description\":\"Browser-local execution observation, distinct from server plugin availability.\\nUnknown means the supervisor has no current confirmation of the session's execution.\",\"enum\":[\"active\",\"inactive\",\"failed\",\"unknown\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"session_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"activation_id\",\"session_id\",\"observed\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"operations\":{\"items\":{\"additionalProperties\":false,\"description\":\"Durable operation evidence; diagnostics can retain lingering physical work after logical success.\",\"properties\":{\"error\":{\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"operation\":{\"description\":\"Administrative operation categories persisted for reconciliation.\",\"enum\":[\"install\",\"activate\",\"deactivate\",\"update\",\"uninstall\",\"configure_provider\",\"recover\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"status\":{\"description\":\"Administrative progress, distinct from plugin availability.\",\"enum\":[\"pending\",\"running\",\"succeeded\",\"failed\",\"interrupted\"],\"type\":\"string\"}},\"required\":[\"id\",\"plugin_id\",\"operation\",\"status\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"plugins\":{\"items\":{\"additionalProperties\":false,\"description\":\"Installed release and its independently tracked intent and observation.\",\"properties\":{\"desired\":{\"description\":\"Durable administrative intent.\",\"enum\":[\"installed\",\"active\",\"absent\"],\"type\":\"string\"},\"errors\":{\"items\":{\"type\":\"string\"},\"type\":\"array\"},\"kind\":{\"description\":\"How a plugin enters the runtime.\",\"enum\":[\"core\",\"dynamic\"],\"type\":\"string\"},\"observed\":{\"description\":\"Sanitized server and administrative availability on the connected node.\\nBrowser execution is reported separately for each browser session. Inactive concerns\\nlogical admission; lingering physical execution remains explicit in operation diagnostics.\",\"enum\":[\"blocked\",\"installed\",\"active\",\"inactive\",\"failed\",\"unavailable\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"provider_id\",\"kind\",\"desired\",\"observed\",\"errors\"],\"type\":\"object\"},\"type\":\"array\"},\"providers\":{\"items\":{\"additionalProperties\":false,\"description\":\"A discovered source provider and its administrative settings.\",\"properties\":{\"configuration\":{},\"operations\":{\"items\":{\"description\":\"Source-provider operation names used in registry metadata and checked schemas.\",\"enum\":[\"search\",\"resolve\",\"inspect\",\"acquire\"],\"type\":\"string\"},\"type\":\"array\"},\"provider_id\":{\"type\":\"string\"}},\"required\":[\"provider_id\",\"configuration\",\"operations\"],\"type\":\"object\"},\"type\":\"array\"}},\"required\":[\"plugins\",\"providers\",\"operations\",\"browser_sessions\"],\"title\":\"Inventory\",\"type\":\"object\"}},\"inventory\":{\"args\":{\"additionalProperties\":false,\"description\":\"An authenticated inventory query.\",\"properties\":{\"proof\":{\"description\":\"Opaque administrative capability material, verified by the receiving supervisor.\\n\\nConstruction only wraps a value. It does not authenticate a caller or grant permission.\\nDebug output is redacted; serialization intentionally carries the proof to its verifier.\",\"type\":\"string\"}},\"required\":[\"proof\"],\"title\":\"InventoryRequest\",\"type\":\"object\"},\"kind\":\"method\",\"result\":{\"additionalProperties\":false,\"description\":\"Sanitized administrative snapshot rebuilt from durable records and live discovery.\",\"properties\":{\"browser_sessions\":{\"items\":{\"additionalProperties\":false,\"description\":\"Persisted browser session evidence; an empty error denotes no reported error.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"error\":{\"type\":\"string\"},\"observed\":{\"description\":\"Browser-local execution observation, distinct from server plugin availability.\\nUnknown means the supervisor has no current confirmation of the session's execution.\",\"enum\":[\"active\",\"inactive\",\"failed\",\"unknown\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"session_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"activation_id\",\"session_id\",\"observed\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"operations\":{\"items\":{\"additionalProperties\":false,\"description\":\"Durable operation evidence; diagnostics can retain lingering physical work after logical success.\",\"properties\":{\"error\":{\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"operation\":{\"description\":\"Administrative operation categories persisted for reconciliation.\",\"enum\":[\"install\",\"activate\",\"deactivate\",\"update\",\"uninstall\",\"configure_provider\",\"recover\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"status\":{\"description\":\"Administrative progress, distinct from plugin availability.\",\"enum\":[\"pending\",\"running\",\"succeeded\",\"failed\",\"interrupted\"],\"type\":\"string\"}},\"required\":[\"id\",\"plugin_id\",\"operation\",\"status\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"plugins\":{\"items\":{\"additionalProperties\":false,\"description\":\"Installed release and its independently tracked intent and observation.\",\"properties\":{\"desired\":{\"description\":\"Durable administrative intent.\",\"enum\":[\"installed\",\"active\",\"absent\"],\"type\":\"string\"},\"errors\":{\"items\":{\"type\":\"string\"},\"type\":\"array\"},\"kind\":{\"description\":\"How a plugin enters the runtime.\",\"enum\":[\"core\",\"dynamic\"],\"type\":\"string\"},\"observed\":{\"description\":\"Sanitized server and administrative availability on the connected node.\\nBrowser execution is reported separately for each browser session. Inactive concerns\\nlogical admission; lingering physical execution remains explicit in operation diagnostics.\",\"enum\":[\"blocked\",\"installed\",\"active\",\"inactive\",\"failed\",\"unavailable\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"provider_id\",\"kind\",\"desired\",\"observed\",\"errors\"],\"type\":\"object\"},\"type\":\"array\"},\"providers\":{\"items\":{\"additionalProperties\":false,\"description\":\"A discovered source provider and its administrative settings.\",\"properties\":{\"configuration\":{},\"operations\":{\"items\":{\"description\":\"Source-provider operation names used in registry metadata and checked schemas.\",\"enum\":[\"search\",\"resolve\",\"inspect\",\"acquire\"],\"type\":\"string\"},\"type\":\"array\"},\"provider_id\":{\"type\":\"string\"}},\"required\":[\"provider_id\",\"configuration\",\"operations\"],\"type\":\"object\"},\"type\":\"array\"}},\"required\":[\"plugins\",\"providers\",\"operations\",\"browser_sessions\"],\"title\":\"Inventory\",\"type\":\"object\"}},\"read_package_file\":{\"args\":{\"additionalProperties\":false,\"description\":\"A receipt-bound request for at most 32 KiB of an approved package file.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"length\":{\"maximum\":32768,\"minimum\":1,\"type\":\"integer\"},\"offset\":{\"maximum\":8388608,\"minimum\":0,\"type\":\"integer\"},\"path\":{\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"proof\":{\"description\":\"Opaque administrative capability material, verified by the receiving supervisor.\\n\\nConstruction only wraps a value. It does not authenticate a caller or grant permission.\\nDebug output is redacted; serialization intentionally carries the proof to its verifier.\",\"type\":\"string\"}},\"required\":[\"proof\",\"plugin_id\",\"activation_id\",\"path\",\"offset\",\"length\"],\"title\":\"ReadPackageFileRequest\",\"type\":\"object\"},\"kind\":\"method\",\"result\":{\"additionalProperties\":false,\"description\":\"A bounded exact byte range and the stable approved file length.\",\"properties\":{\"bytes\":{\"items\":{\"maximum\":255,\"minimum\":0,\"type\":\"integer\"},\"maxItems\":32768,\"type\":\"array\"},\"total_length\":{\"maximum\":8388608,\"minimum\":0,\"type\":\"integer\"}},\"required\":[\"bytes\",\"total_length\"],\"title\":\"ReadPackageFileResult\",\"type\":\"object\"}},\"report_browser\":{\"args\":{\"additionalProperties\":false,\"description\":\"An authenticated browser acknowledgement bound to a specific activation and session.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"error\":{\"type\":\"string\"},\"observed\":{\"description\":\"Browser-local execution observation, distinct from server plugin availability.\\nUnknown means the supervisor has no current confirmation of the session's execution.\",\"enum\":[\"active\",\"inactive\",\"failed\",\"unknown\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"proof\":{\"description\":\"Opaque administrative capability material, verified by the receiving supervisor.\\n\\nConstruction only wraps a value. It does not authenticate a caller or grant permission.\\nDebug output is redacted; serialization intentionally carries the proof to its verifier.\",\"type\":\"string\"},\"session_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"proof\",\"plugin_id\",\"version\",\"activation_id\",\"session_id\",\"observed\",\"error\"],\"title\":\"ReportBrowserRequest\",\"type\":\"object\"},\"kind\":\"method\",\"result\":{\"additionalProperties\":false,\"description\":\"Sanitized administrative snapshot rebuilt from durable records and live discovery.\",\"properties\":{\"browser_sessions\":{\"items\":{\"additionalProperties\":false,\"description\":\"Persisted browser session evidence; an empty error denotes no reported error.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"error\":{\"type\":\"string\"},\"observed\":{\"description\":\"Browser-local execution observation, distinct from server plugin availability.\\nUnknown means the supervisor has no current confirmation of the session's execution.\",\"enum\":[\"active\",\"inactive\",\"failed\",\"unknown\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"session_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"activation_id\",\"session_id\",\"observed\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"operations\":{\"items\":{\"additionalProperties\":false,\"description\":\"Durable operation evidence; diagnostics can retain lingering physical work after logical success.\",\"properties\":{\"error\":{\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"operation\":{\"description\":\"Administrative operation categories persisted for reconciliation.\",\"enum\":[\"install\",\"activate\",\"deactivate\",\"update\",\"uninstall\",\"configure_provider\",\"recover\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"status\":{\"description\":\"Administrative progress, distinct from plugin availability.\",\"enum\":[\"pending\",\"running\",\"succeeded\",\"failed\",\"interrupted\"],\"type\":\"string\"}},\"required\":[\"id\",\"plugin_id\",\"operation\",\"status\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"plugins\":{\"items\":{\"additionalProperties\":false,\"description\":\"Installed release and its independently tracked intent and observation.\",\"properties\":{\"desired\":{\"description\":\"Durable administrative intent.\",\"enum\":[\"installed\",\"active\",\"absent\"],\"type\":\"string\"},\"errors\":{\"items\":{\"type\":\"string\"},\"type\":\"array\"},\"kind\":{\"description\":\"How a plugin enters the runtime.\",\"enum\":[\"core\",\"dynamic\"],\"type\":\"string\"},\"observed\":{\"description\":\"Sanitized server and administrative availability on the connected node.\\nBrowser execution is reported separately for each browser session. Inactive concerns\\nlogical admission; lingering physical execution remains explicit in operation diagnostics.\",\"enum\":[\"blocked\",\"installed\",\"active\",\"inactive\",\"failed\",\"unavailable\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"provider_id\",\"kind\",\"desired\",\"observed\",\"errors\"],\"type\":\"object\"},\"type\":\"array\"},\"providers\":{\"items\":{\"additionalProperties\":false,\"description\":\"A discovered source provider and its administrative settings.\",\"properties\":{\"configuration\":{},\"operations\":{\"items\":{\"description\":\"Source-provider operation names used in registry metadata and checked schemas.\",\"enum\":[\"search\",\"resolve\",\"inspect\",\"acquire\"],\"type\":\"string\"},\"type\":\"array\"},\"provider_id\":{\"type\":\"string\"}},\"required\":[\"provider_id\",\"configuration\",\"operations\"],\"type\":\"object\"},\"type\":\"array\"}},\"required\":[\"plugins\",\"providers\",\"operations\",\"browser_sessions\"],\"title\":\"Inventory\",\"type\":\"object\"}},\"uninstall\":{\"args\":{\"additionalProperties\":false,\"description\":\"An installed plugin selected for removal.\",\"properties\":{\"plugin_id\":{\"type\":\"string\"},\"proof\":{\"description\":\"Opaque administrative capability material, verified by the receiving supervisor.\\n\\nConstruction only wraps a value. It does not authenticate a caller or grant permission.\\nDebug output is redacted; serialization intentionally carries the proof to its verifier.\",\"type\":\"string\"}},\"required\":[\"proof\",\"plugin_id\"],\"title\":\"UninstallRequest\",\"type\":\"object\"},\"kind\":\"method\",\"result\":{\"additionalProperties\":false,\"description\":\"Sanitized administrative snapshot rebuilt from durable records and live discovery.\",\"properties\":{\"browser_sessions\":{\"items\":{\"additionalProperties\":false,\"description\":\"Persisted browser session evidence; an empty error denotes no reported error.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"error\":{\"type\":\"string\"},\"observed\":{\"description\":\"Browser-local execution observation, distinct from server plugin availability.\\nUnknown means the supervisor has no current confirmation of the session's execution.\",\"enum\":[\"active\",\"inactive\",\"failed\",\"unknown\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"session_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"activation_id\",\"session_id\",\"observed\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"operations\":{\"items\":{\"additionalProperties\":false,\"description\":\"Durable operation evidence; diagnostics can retain lingering physical work after logical success.\",\"properties\":{\"error\":{\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"operation\":{\"description\":\"Administrative operation categories persisted for reconciliation.\",\"enum\":[\"install\",\"activate\",\"deactivate\",\"update\",\"uninstall\",\"configure_provider\",\"recover\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"status\":{\"description\":\"Administrative progress, distinct from plugin availability.\",\"enum\":[\"pending\",\"running\",\"succeeded\",\"failed\",\"interrupted\"],\"type\":\"string\"}},\"required\":[\"id\",\"plugin_id\",\"operation\",\"status\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"plugins\":{\"items\":{\"additionalProperties\":false,\"description\":\"Installed release and its independently tracked intent and observation.\",\"properties\":{\"desired\":{\"description\":\"Durable administrative intent.\",\"enum\":[\"installed\",\"active\",\"absent\"],\"type\":\"string\"},\"errors\":{\"items\":{\"type\":\"string\"},\"type\":\"array\"},\"kind\":{\"description\":\"How a plugin enters the runtime.\",\"enum\":[\"core\",\"dynamic\"],\"type\":\"string\"},\"observed\":{\"description\":\"Sanitized server and administrative availability on the connected node.\\nBrowser execution is reported separately for each browser session. Inactive concerns\\nlogical admission; lingering physical execution remains explicit in operation diagnostics.\",\"enum\":[\"blocked\",\"installed\",\"active\",\"inactive\",\"failed\",\"unavailable\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"provider_id\",\"kind\",\"desired\",\"observed\",\"errors\"],\"type\":\"object\"},\"type\":\"array\"},\"providers\":{\"items\":{\"additionalProperties\":false,\"description\":\"A discovered source provider and its administrative settings.\",\"properties\":{\"configuration\":{},\"operations\":{\"items\":{\"description\":\"Source-provider operation names used in registry metadata and checked schemas.\",\"enum\":[\"search\",\"resolve\",\"inspect\",\"acquire\"],\"type\":\"string\"},\"type\":\"array\"},\"provider_id\":{\"type\":\"string\"}},\"required\":[\"provider_id\",\"configuration\",\"operations\"],\"type\":\"object\"},\"type\":\"array\"}},\"required\":[\"plugins\",\"providers\",\"operations\",\"browser_sessions\"],\"title\":\"Inventory\",\"type\":\"object\"}},\"update\":{\"args\":{\"additionalProperties\":false,\"description\":\"An explicitly confirmed replacement for an installed plugin.\",\"properties\":{\"confirmed\":{\"type\":\"boolean\"},\"plugin_id\":{\"type\":\"string\"},\"proof\":{\"description\":\"Opaque administrative capability material, verified by the receiving supervisor.\\n\\nConstruction only wraps a value. It does not authenticate a caller or grant permission.\\nDebug output is redacted; serialization intentionally carries the proof to its verifier.\",\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"proof\",\"provider_id\",\"plugin_id\",\"version\",\"confirmed\"],\"title\":\"UpdateRequest\",\"type\":\"object\"},\"kind\":\"method\",\"result\":{\"additionalProperties\":false,\"description\":\"Sanitized administrative snapshot rebuilt from durable records and live discovery.\",\"properties\":{\"browser_sessions\":{\"items\":{\"additionalProperties\":false,\"description\":\"Persisted browser session evidence; an empty error denotes no reported error.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"error\":{\"type\":\"string\"},\"observed\":{\"description\":\"Browser-local execution observation, distinct from server plugin availability.\\nUnknown means the supervisor has no current confirmation of the session's execution.\",\"enum\":[\"active\",\"inactive\",\"failed\",\"unknown\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"session_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"activation_id\",\"session_id\",\"observed\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"operations\":{\"items\":{\"additionalProperties\":false,\"description\":\"Durable operation evidence; diagnostics can retain lingering physical work after logical success.\",\"properties\":{\"error\":{\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"operation\":{\"description\":\"Administrative operation categories persisted for reconciliation.\",\"enum\":[\"install\",\"activate\",\"deactivate\",\"update\",\"uninstall\",\"configure_provider\",\"recover\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"status\":{\"description\":\"Administrative progress, distinct from plugin availability.\",\"enum\":[\"pending\",\"running\",\"succeeded\",\"failed\",\"interrupted\"],\"type\":\"string\"}},\"required\":[\"id\",\"plugin_id\",\"operation\",\"status\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"plugins\":{\"items\":{\"additionalProperties\":false,\"description\":\"Installed release and its independently tracked intent and observation.\",\"properties\":{\"desired\":{\"description\":\"Durable administrative intent.\",\"enum\":[\"installed\",\"active\",\"absent\"],\"type\":\"string\"},\"errors\":{\"items\":{\"type\":\"string\"},\"type\":\"array\"},\"kind\":{\"description\":\"How a plugin enters the runtime.\",\"enum\":[\"core\",\"dynamic\"],\"type\":\"string\"},\"observed\":{\"description\":\"Sanitized server and administrative availability on the connected node.\\nBrowser execution is reported separately for each browser session. Inactive concerns\\nlogical admission; lingering physical execution remains explicit in operation diagnostics.\",\"enum\":[\"blocked\",\"installed\",\"active\",\"inactive\",\"failed\",\"unavailable\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"provider_id\",\"kind\",\"desired\",\"observed\",\"errors\"],\"type\":\"object\"},\"type\":\"array\"},\"providers\":{\"items\":{\"additionalProperties\":false,\"description\":\"A discovered source provider and its administrative settings.\",\"properties\":{\"configuration\":{},\"operations\":{\"items\":{\"description\":\"Source-provider operation names used in registry metadata and checked schemas.\",\"enum\":[\"search\",\"resolve\",\"inspect\",\"acquire\"],\"type\":\"string\"},\"type\":\"array\"},\"provider_id\":{\"type\":\"string\"}},\"required\":[\"provider_id\",\"configuration\",\"operations\"],\"type\":\"object\"},\"type\":\"array\"}},\"required\":[\"plugins\",\"providers\",\"operations\",\"browser_sessions\"],\"title\":\"Inventory\",\"type\":\"object\"}}}},\"version\":1}")) as GeneratedContract<RemotePluginAdministrationInput, RemotePluginAdministrationReference>;

export function bindRemotePluginAdministration(port: BindingPort): RemotePluginAdministrationReference {
  return bindReference(RemotePluginAdministration, port);
}

export type RemotePluginAdministrationAcquisitionInput = RemotePluginAdministrationInput;
