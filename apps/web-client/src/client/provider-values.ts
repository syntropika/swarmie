import type {
  ProviderAccount,
  ProviderChoice,
  ProviderModel,
  ProviderConfigurationMethod,
  ModelDefaults,
} from "@swarmie/plugin-providers-browser";

function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function text(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.trim().length > 0 &&
    new TextEncoder().encode(value).length <= 512
  );
}
function array(value: unknown): value is unknown[] {
  return Array.isArray(value) && value.length <= 512;
}
export function modelDefaults(value: unknown): ModelDefaults {
  if (
    !object(value) ||
    (value.model !== null && !text(value.model)) ||
    (value.effort !== null && !text(value.effort)) ||
    (value.auxiliaryModel !== null && !text(value.auxiliaryModel))
  ) {
    throw new Error("The node returned invalid model defaults.");
  }
  return { model: value.model, effort: value.effort, auxiliaryModel: value.auxiliaryModel };
}
function model(value: unknown): ProviderModel {
  if (
    !object(value) ||
    !text(value.id) ||
    new TextEncoder().encode(value.id).length > 255 ||
    !text(value.label) ||
    !array(value.efforts)
  ) {
    throw new Error("The node returned an invalid model.");
  }
  const efforts = value.efforts.map((effort) => {
    if (!text(effort)) {
      throw new Error("The node returned invalid reasoning levels.");
    }
    return effort;
  });
  return { id: value.id, label: value.label, efforts };
}
export function providerAccount(value: unknown): ProviderAccount {
  if (
    !object(value) ||
    !text(value.id) ||
    !text(value.providerId) ||
    !text(value.providerLabel) ||
    !text(value.label) ||
    typeof value.available !== "boolean" ||
    (value.modelsTruncated !== undefined && typeof value.modelsTruncated !== "boolean") ||
    (value.authorizationMethod !== undefined && !text(value.authorizationMethod)) ||
    !array(value.models)
  ) {
    throw new Error("The node returned an invalid provider account.");
  }
  return {
    id: value.id,
    providerId: value.providerId,
    providerLabel: value.providerLabel,
    label: value.label,
    available: value.available,
    models: value.models.map(model),
    ...(value.defaults === undefined ? {} : { defaults: modelDefaults(value.defaults) }),
    ...(value.modelsTruncated === true ? { modelsTruncated: true } : {}),
    ...(value.authorizationMethod === undefined
      ? {}
      : { authorizationMethod: value.authorizationMethod }),
  };
}
function choice(value: unknown): ProviderChoice {
  if (
    !object(value) ||
    !text(value.id) ||
    !text(value.label) ||
    !text(value.publisher) ||
    !array(value.methods)
  ) {
    throw new Error("The node returned an invalid provider choice.");
  }
  const methods = value.methods.map<ProviderConfigurationMethod>((method) => {
    if (
      !object(method) ||
      !text(method.id) ||
      !text(method.label) ||
      (method.kind !== "api-key" && method.kind !== "local" && method.kind !== "oauth")
    ) {
      throw new Error("The node returned an invalid connection method.");
    }
    return { id: method.id, label: method.label, kind: method.kind };
  });
  return { id: value.id, label: value.label, publisher: value.publisher, methods };
}
export function providerCatalog(value: unknown) {
  if (!object(value) || !array(value.choices) || !array(value.accounts)) {
    throw new Error("The node returned an invalid provider catalog.");
  }
  if (
    value.nextCursor !== null &&
    (typeof value.nextCursor !== "string" ||
      value.nextCursor.length > 512 ||
      value.nextCursor.length === 0)
  ) {
    throw new Error("The node returned an invalid provider cursor.");
  }
  return {
    choices: value.choices.map(choice),
    accounts: value.accounts.map(providerAccount),
    nextCursor: value.nextCursor,
  };
}
