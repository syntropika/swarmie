// Generated from a verified Tembra descriptor. Do not edit.

import { bindReference, fromDescriptor, parsePortableJson, type BindingPort, type ContextHandle, type ContractReference, type GeneratedContract, type Readable } from "@tembra/sdk";

export interface RemoteBrowserAccessInput {
  [k: string]: unknown | undefined;
}


export interface RemoteBrowserAccessMainActivePackageArgs {
  /**
   * Opaque, revocable browser capability issued by the trusted transport embedding.
   *
   * A grant is bound to one supervisor incarnation, admitted session and exact approved
   * activations. It never authorizes administrative mutation. The embedding must authenticate
   * its session before issuing a grant; a caller-supplied session label is not authentication.
   */
  grant: string;
  plugin_id: string;
  session_id: string;
}


/**
 * Approved package metadata and the activation receipt required for bounded file reads.
 */
export interface RemoteBrowserAccessMainActivePackageResult {
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


export interface RemoteBrowserAccessMainInventoryArgs {
  /**
   * Opaque, revocable browser capability issued by the trusted transport embedding.
   *
   * A grant is bound to one supervisor incarnation, admitted session and exact approved
   * activations. It never authorizes administrative mutation. The embedding must authenticate
   * its session before issuing a grant; a caller-supplied session label is not authentication.
   */
  grant: string;
  session_id: string;
}


/**
 * Inventory restricted to this grant's approved packages and original browser session.
 */
export interface RemoteBrowserAccessMainInventoryResult {
  /**
   * Items: Persisted browser session evidence; an empty error denotes no reported error.
   */
  observations: {
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
   * Items: Approved package metadata and the activation receipt required for bounded file reads.
   */
  packages: {
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
  }[];
  session_id: string;
}


export interface RemoteBrowserAccessMainReadPackageFileArgs {
  activation_id: string;
  /**
   * Opaque, revocable browser capability issued by the trusted transport embedding.
   *
   * A grant is bound to one supervisor incarnation, admitted session and exact approved
   * activations. It never authorizes administrative mutation. The embedding must authenticate
   * its session before issuing a grant; a caller-supplied session label is not authentication.
   */
  grant: string;
  length: number;
  offset: number;
  path: string;
  plugin_id: string;
  session_id: string;
}


/**
 * A bounded exact byte range and the stable approved file length.
 */
export interface RemoteBrowserAccessMainReadPackageFileResult {
  /**
   * @maxItems 32768
   */
  bytes: number[];
  total_length: number;
}


export interface RemoteBrowserAccessMainReportBrowserArgs {
  activation_id: string;
  error: string;
  /**
   * Opaque, revocable browser capability issued by the trusted transport embedding.
   *
   * A grant is bound to one supervisor incarnation, admitted session and exact approved
   * activations. It never authorizes administrative mutation. The embedding must authenticate
   * its session before issuing a grant; a caller-supplied session label is not authentication.
   */
  grant: string;
  /**
   * Browser-local execution observation, distinct from server plugin availability.
   * Unknown means the supervisor has no current confirmation of the session's execution.
   */
  observed: "active" | "inactive" | "failed" | "unknown";
  plugin_id: string;
  session_id: string;
  version: string;
}


/**
 * Inventory restricted to this grant's approved packages and original browser session.
 */
export interface RemoteBrowserAccessMainReportBrowserResult {
  /**
   * Items: Persisted browser session evidence; an empty error denotes no reported error.
   */
  observations: {
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
   * Items: Approved package metadata and the activation receipt required for bounded file reads.
   */
  packages: {
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
  }[];
  session_id: string;
}


export interface RemoteBrowserAccessMainReference extends ContractReference {
  readonly variant: "main" | undefined;
  readonly "active_package": (args: RemoteBrowserAccessMainActivePackageArgs, context?: ContextHandle) => Promise<RemoteBrowserAccessMainActivePackageResult>;
  readonly "inventory": (args: RemoteBrowserAccessMainInventoryArgs, context?: ContextHandle) => Promise<RemoteBrowserAccessMainInventoryResult>;
  readonly "read_package_file": (args: RemoteBrowserAccessMainReadPackageFileArgs, context?: ContextHandle) => Promise<RemoteBrowserAccessMainReadPackageFileResult>;
  readonly "report_browser": (args: RemoteBrowserAccessMainReportBrowserArgs, context?: ContextHandle) => Promise<RemoteBrowserAccessMainReportBrowserResult>;
}

export type RemoteBrowserAccessReference = RemoteBrowserAccessMainReference;

export const RemoteBrowserAccess = fromDescriptor(parsePortableJson("{\"descriptorVersion\":1,\"fingerprint\":\"971b8043017b9d3939ac32d723e7b1882be372e3865b753c7ea97ceee55c72f9\",\"input\":{\"title\":\"RemoteBrowserAccessInput\",\"type\":\"object\"},\"kind\":\"remote\",\"name\":\"swarmie.remote-browser-access\",\"placement\":{\"name\":\"swarmie-runtime\"},\"variants\":{\"main\":{\"active_package\":{\"args\":{\"additionalProperties\":false,\"properties\":{\"grant\":{\"description\":\"Opaque, revocable browser capability issued by the trusted transport embedding.\\n\\nA grant is bound to one supervisor incarnation, admitted session and exact approved\\nactivations. It never authorizes administrative mutation. The embedding must authenticate\\nits session before issuing a grant; a caller-supplied session label is not authentication.\",\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"session_id\":{\"type\":\"string\"}},\"required\":[\"grant\",\"session_id\",\"plugin_id\"],\"title\":\"BrowserActivePackageRequest\",\"type\":\"object\"},\"kind\":\"method\",\"result\":{\"additionalProperties\":false,\"description\":\"Approved package metadata and the activation receipt required for bounded file reads.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"manifest\":{\"additionalProperties\":false,\"description\":\"The plugin metadata consumed by administration and generated browser bindings.\",\"properties\":{\"compatibility\":{\"additionalProperties\":false,\"description\":\"Runtime and dependency requirements, checked before admitting plugin artifacts.\",\"properties\":{\"swarmie\":{\"type\":\"string\"},\"tembra_revision\":{\"type\":\"string\"}},\"required\":[\"swarmie\",\"tembra_revision\"],\"type\":\"object\"},\"dependencies\":{\"items\":{\"additionalProperties\":false,\"description\":\"A required plugin version range.\",\"properties\":{\"id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"id\",\"version\"],\"type\":\"object\"},\"type\":\"array\"},\"distribution\":{\"additionalProperties\":false,\"description\":\"Human-readable distribution metadata; this does not select a manager implementation.\",\"properties\":{\"description\":{\"type\":\"string\"},\"provider\":{\"type\":\"string\"}},\"required\":[\"provider\",\"description\"],\"type\":\"object\"},\"facets\":{\"items\":{\"additionalProperties\":false,\"description\":\"A verified Tembra artifact and its selected module export.\",\"properties\":{\"artifact_path\":{\"type\":\"string\"},\"format\":{\"description\":\"Supported Tembra loading mechanisms; native browser ESM is not embedded server code.\",\"enum\":[\"bundled\",\"native_esm\"],\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"selected_export\":{\"type\":\"string\"},\"sha256\":{\"type\":\"string\"},\"target\":{\"description\":\"Supported JavaScript execution locations.\",\"enum\":[\"server\",\"browser\"],\"type\":\"string\"}},\"required\":[\"id\",\"target\",\"artifact_path\",\"sha256\",\"selected_export\"],\"type\":\"object\"},\"type\":\"array\"},\"files\":{\"items\":{\"additionalProperties\":false,\"description\":\"An integrity-pinned file relative to the acquired package root.\",\"properties\":{\"path\":{\"type\":\"string\"},\"sha256\":{\"type\":\"string\"}},\"required\":[\"path\",\"sha256\"],\"type\":\"object\"},\"type\":\"array\"},\"id\":{\"type\":\"string\"},\"schema_version\":{\"maximum\":4294967295,\"minimum\":0,\"type\":\"integer\"},\"ui\":{\"items\":{\"additionalProperties\":false,\"description\":\"A lifecycle-owned browser module export, resolved by identity rather than serialized code.\",\"properties\":{\"export_name\":{\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"kind\":{\"description\":\"Supported browser contribution categories.\",\"enum\":[\"slot\",\"widget\",\"command\",\"panel\",\"screen\",\"renderer\",\"configuration\"],\"type\":\"string\"},\"label\":{\"type\":\"string\"},\"module_path\":{\"type\":\"string\"},\"order\":{\"maximum\":2147483647,\"minimum\":-2147483648,\"type\":\"integer\"},\"plugin_id\":{\"type\":\"string\"},\"slot_id\":{\"type\":\"string\"}},\"required\":[\"id\",\"plugin_id\",\"kind\",\"label\",\"slot_id\",\"order\",\"module_path\",\"export_name\"],\"type\":\"object\"},\"type\":\"array\"},\"version\":{\"type\":\"string\"}},\"required\":[\"schema_version\",\"id\",\"version\",\"compatibility\",\"dependencies\",\"facets\",\"files\",\"distribution\",\"ui\"],\"type\":\"object\"},\"provenance\":{\"additionalProperties\":false,\"description\":\"Provider-issued acquisition evidence, retained with durable administrative records.\",\"properties\":{\"integrity\":{\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"},\"source\":{\"type\":\"string\"}},\"required\":[\"provider_id\",\"source\",\"integrity\"],\"type\":\"object\"}},\"required\":[\"manifest\",\"provenance\",\"activation_id\"],\"title\":\"ActivePackage\",\"type\":\"object\"}},\"inventory\":{\"args\":{\"additionalProperties\":false,\"properties\":{\"grant\":{\"description\":\"Opaque, revocable browser capability issued by the trusted transport embedding.\\n\\nA grant is bound to one supervisor incarnation, admitted session and exact approved\\nactivations. It never authorizes administrative mutation. The embedding must authenticate\\nits session before issuing a grant; a caller-supplied session label is not authentication.\",\"type\":\"string\"},\"session_id\":{\"type\":\"string\"}},\"required\":[\"grant\",\"session_id\"],\"title\":\"BrowserInventoryRequest\",\"type\":\"object\"},\"kind\":\"method\",\"result\":{\"additionalProperties\":false,\"description\":\"Inventory restricted to this grant's approved packages and original browser session.\",\"properties\":{\"observations\":{\"items\":{\"additionalProperties\":false,\"description\":\"Persisted browser session evidence; an empty error denotes no reported error.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"error\":{\"type\":\"string\"},\"observed\":{\"description\":\"Browser-local execution observation, distinct from server plugin availability.\\nUnknown means the supervisor has no current confirmation of the session's execution.\",\"enum\":[\"active\",\"inactive\",\"failed\",\"unknown\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"session_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"activation_id\",\"session_id\",\"observed\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"packages\":{\"items\":{\"additionalProperties\":false,\"description\":\"Approved package metadata and the activation receipt required for bounded file reads.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"manifest\":{\"additionalProperties\":false,\"description\":\"The plugin metadata consumed by administration and generated browser bindings.\",\"properties\":{\"compatibility\":{\"additionalProperties\":false,\"description\":\"Runtime and dependency requirements, checked before admitting plugin artifacts.\",\"properties\":{\"swarmie\":{\"type\":\"string\"},\"tembra_revision\":{\"type\":\"string\"}},\"required\":[\"swarmie\",\"tembra_revision\"],\"type\":\"object\"},\"dependencies\":{\"items\":{\"additionalProperties\":false,\"description\":\"A required plugin version range.\",\"properties\":{\"id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"id\",\"version\"],\"type\":\"object\"},\"type\":\"array\"},\"distribution\":{\"additionalProperties\":false,\"description\":\"Human-readable distribution metadata; this does not select a manager implementation.\",\"properties\":{\"description\":{\"type\":\"string\"},\"provider\":{\"type\":\"string\"}},\"required\":[\"provider\",\"description\"],\"type\":\"object\"},\"facets\":{\"items\":{\"additionalProperties\":false,\"description\":\"A verified Tembra artifact and its selected module export.\",\"properties\":{\"artifact_path\":{\"type\":\"string\"},\"format\":{\"description\":\"Supported Tembra loading mechanisms; native browser ESM is not embedded server code.\",\"enum\":[\"bundled\",\"native_esm\"],\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"selected_export\":{\"type\":\"string\"},\"sha256\":{\"type\":\"string\"},\"target\":{\"description\":\"Supported JavaScript execution locations.\",\"enum\":[\"server\",\"browser\"],\"type\":\"string\"}},\"required\":[\"id\",\"target\",\"artifact_path\",\"sha256\",\"selected_export\"],\"type\":\"object\"},\"type\":\"array\"},\"files\":{\"items\":{\"additionalProperties\":false,\"description\":\"An integrity-pinned file relative to the acquired package root.\",\"properties\":{\"path\":{\"type\":\"string\"},\"sha256\":{\"type\":\"string\"}},\"required\":[\"path\",\"sha256\"],\"type\":\"object\"},\"type\":\"array\"},\"id\":{\"type\":\"string\"},\"schema_version\":{\"maximum\":4294967295,\"minimum\":0,\"type\":\"integer\"},\"ui\":{\"items\":{\"additionalProperties\":false,\"description\":\"A lifecycle-owned browser module export, resolved by identity rather than serialized code.\",\"properties\":{\"export_name\":{\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"kind\":{\"description\":\"Supported browser contribution categories.\",\"enum\":[\"slot\",\"widget\",\"command\",\"panel\",\"screen\",\"renderer\",\"configuration\"],\"type\":\"string\"},\"label\":{\"type\":\"string\"},\"module_path\":{\"type\":\"string\"},\"order\":{\"maximum\":2147483647,\"minimum\":-2147483648,\"type\":\"integer\"},\"plugin_id\":{\"type\":\"string\"},\"slot_id\":{\"type\":\"string\"}},\"required\":[\"id\",\"plugin_id\",\"kind\",\"label\",\"slot_id\",\"order\",\"module_path\",\"export_name\"],\"type\":\"object\"},\"type\":\"array\"},\"version\":{\"type\":\"string\"}},\"required\":[\"schema_version\",\"id\",\"version\",\"compatibility\",\"dependencies\",\"facets\",\"files\",\"distribution\",\"ui\"],\"type\":\"object\"},\"provenance\":{\"additionalProperties\":false,\"description\":\"Provider-issued acquisition evidence, retained with durable administrative records.\",\"properties\":{\"integrity\":{\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"},\"source\":{\"type\":\"string\"}},\"required\":[\"provider_id\",\"source\",\"integrity\"],\"type\":\"object\"}},\"required\":[\"manifest\",\"provenance\",\"activation_id\"],\"type\":\"object\"},\"type\":\"array\"},\"session_id\":{\"type\":\"string\"}},\"required\":[\"session_id\",\"packages\",\"observations\"],\"title\":\"BrowserInventory\",\"type\":\"object\"}},\"read_package_file\":{\"args\":{\"additionalProperties\":false,\"properties\":{\"activation_id\":{\"type\":\"string\"},\"grant\":{\"description\":\"Opaque, revocable browser capability issued by the trusted transport embedding.\\n\\nA grant is bound to one supervisor incarnation, admitted session and exact approved\\nactivations. It never authorizes administrative mutation. The embedding must authenticate\\nits session before issuing a grant; a caller-supplied session label is not authentication.\",\"type\":\"string\"},\"length\":{\"maximum\":32768,\"minimum\":1,\"type\":\"integer\"},\"offset\":{\"maximum\":8388608,\"minimum\":0,\"type\":\"integer\"},\"path\":{\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"session_id\":{\"type\":\"string\"}},\"required\":[\"grant\",\"session_id\",\"plugin_id\",\"activation_id\",\"path\",\"offset\",\"length\"],\"title\":\"BrowserReadPackageFileRequest\",\"type\":\"object\"},\"kind\":\"method\",\"result\":{\"additionalProperties\":false,\"description\":\"A bounded exact byte range and the stable approved file length.\",\"properties\":{\"bytes\":{\"items\":{\"maximum\":255,\"minimum\":0,\"type\":\"integer\"},\"maxItems\":32768,\"type\":\"array\"},\"total_length\":{\"maximum\":8388608,\"minimum\":0,\"type\":\"integer\"}},\"required\":[\"bytes\",\"total_length\"],\"title\":\"ReadPackageFileResult\",\"type\":\"object\"}},\"report_browser\":{\"args\":{\"additionalProperties\":false,\"properties\":{\"activation_id\":{\"type\":\"string\"},\"error\":{\"type\":\"string\"},\"grant\":{\"description\":\"Opaque, revocable browser capability issued by the trusted transport embedding.\\n\\nA grant is bound to one supervisor incarnation, admitted session and exact approved\\nactivations. It never authorizes administrative mutation. The embedding must authenticate\\nits session before issuing a grant; a caller-supplied session label is not authentication.\",\"type\":\"string\"},\"observed\":{\"description\":\"Browser-local execution observation, distinct from server plugin availability.\\nUnknown means the supervisor has no current confirmation of the session's execution.\",\"enum\":[\"active\",\"inactive\",\"failed\",\"unknown\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"session_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"grant\",\"session_id\",\"plugin_id\",\"version\",\"activation_id\",\"observed\",\"error\"],\"title\":\"BrowserReportRequest\",\"type\":\"object\"},\"kind\":\"method\",\"result\":{\"additionalProperties\":false,\"description\":\"Inventory restricted to this grant's approved packages and original browser session.\",\"properties\":{\"observations\":{\"items\":{\"additionalProperties\":false,\"description\":\"Persisted browser session evidence; an empty error denotes no reported error.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"error\":{\"type\":\"string\"},\"observed\":{\"description\":\"Browser-local execution observation, distinct from server plugin availability.\\nUnknown means the supervisor has no current confirmation of the session's execution.\",\"enum\":[\"active\",\"inactive\",\"failed\",\"unknown\"],\"type\":\"string\"},\"plugin_id\":{\"type\":\"string\"},\"session_id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"plugin_id\",\"version\",\"activation_id\",\"session_id\",\"observed\",\"error\"],\"type\":\"object\"},\"type\":\"array\"},\"packages\":{\"items\":{\"additionalProperties\":false,\"description\":\"Approved package metadata and the activation receipt required for bounded file reads.\",\"properties\":{\"activation_id\":{\"type\":\"string\"},\"manifest\":{\"additionalProperties\":false,\"description\":\"The plugin metadata consumed by administration and generated browser bindings.\",\"properties\":{\"compatibility\":{\"additionalProperties\":false,\"description\":\"Runtime and dependency requirements, checked before admitting plugin artifacts.\",\"properties\":{\"swarmie\":{\"type\":\"string\"},\"tembra_revision\":{\"type\":\"string\"}},\"required\":[\"swarmie\",\"tembra_revision\"],\"type\":\"object\"},\"dependencies\":{\"items\":{\"additionalProperties\":false,\"description\":\"A required plugin version range.\",\"properties\":{\"id\":{\"type\":\"string\"},\"version\":{\"type\":\"string\"}},\"required\":[\"id\",\"version\"],\"type\":\"object\"},\"type\":\"array\"},\"distribution\":{\"additionalProperties\":false,\"description\":\"Human-readable distribution metadata; this does not select a manager implementation.\",\"properties\":{\"description\":{\"type\":\"string\"},\"provider\":{\"type\":\"string\"}},\"required\":[\"provider\",\"description\"],\"type\":\"object\"},\"facets\":{\"items\":{\"additionalProperties\":false,\"description\":\"A verified Tembra artifact and its selected module export.\",\"properties\":{\"artifact_path\":{\"type\":\"string\"},\"format\":{\"description\":\"Supported Tembra loading mechanisms; native browser ESM is not embedded server code.\",\"enum\":[\"bundled\",\"native_esm\"],\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"selected_export\":{\"type\":\"string\"},\"sha256\":{\"type\":\"string\"},\"target\":{\"description\":\"Supported JavaScript execution locations.\",\"enum\":[\"server\",\"browser\"],\"type\":\"string\"}},\"required\":[\"id\",\"target\",\"artifact_path\",\"sha256\",\"selected_export\"],\"type\":\"object\"},\"type\":\"array\"},\"files\":{\"items\":{\"additionalProperties\":false,\"description\":\"An integrity-pinned file relative to the acquired package root.\",\"properties\":{\"path\":{\"type\":\"string\"},\"sha256\":{\"type\":\"string\"}},\"required\":[\"path\",\"sha256\"],\"type\":\"object\"},\"type\":\"array\"},\"id\":{\"type\":\"string\"},\"schema_version\":{\"maximum\":4294967295,\"minimum\":0,\"type\":\"integer\"},\"ui\":{\"items\":{\"additionalProperties\":false,\"description\":\"A lifecycle-owned browser module export, resolved by identity rather than serialized code.\",\"properties\":{\"export_name\":{\"type\":\"string\"},\"id\":{\"type\":\"string\"},\"kind\":{\"description\":\"Supported browser contribution categories.\",\"enum\":[\"slot\",\"widget\",\"command\",\"panel\",\"screen\",\"renderer\",\"configuration\"],\"type\":\"string\"},\"label\":{\"type\":\"string\"},\"module_path\":{\"type\":\"string\"},\"order\":{\"maximum\":2147483647,\"minimum\":-2147483648,\"type\":\"integer\"},\"plugin_id\":{\"type\":\"string\"},\"slot_id\":{\"type\":\"string\"}},\"required\":[\"id\",\"plugin_id\",\"kind\",\"label\",\"slot_id\",\"order\",\"module_path\",\"export_name\"],\"type\":\"object\"},\"type\":\"array\"},\"version\":{\"type\":\"string\"}},\"required\":[\"schema_version\",\"id\",\"version\",\"compatibility\",\"dependencies\",\"facets\",\"files\",\"distribution\",\"ui\"],\"type\":\"object\"},\"provenance\":{\"additionalProperties\":false,\"description\":\"Provider-issued acquisition evidence, retained with durable administrative records.\",\"properties\":{\"integrity\":{\"type\":\"string\"},\"provider_id\":{\"type\":\"string\"},\"source\":{\"type\":\"string\"}},\"required\":[\"provider_id\",\"source\",\"integrity\"],\"type\":\"object\"}},\"required\":[\"manifest\",\"provenance\",\"activation_id\"],\"type\":\"object\"},\"type\":\"array\"},\"session_id\":{\"type\":\"string\"}},\"required\":[\"session_id\",\"packages\",\"observations\"],\"title\":\"BrowserInventory\",\"type\":\"object\"}}}},\"version\":1}")) as GeneratedContract<RemoteBrowserAccessInput, RemoteBrowserAccessReference>;

export function bindRemoteBrowserAccess(port: BindingPort): RemoteBrowserAccessReference {
  return bindReference(RemoteBrowserAccess, port);
}

export type RemoteBrowserAccessAcquisitionInput = RemoteBrowserAccessInput;
