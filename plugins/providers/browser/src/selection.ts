import type { ProviderAccount, ProviderModel, ProvidersState } from "./index";

/** Portable execution selection; no Conversation or Composer types are required. */
export type ProviderSelection = { account: string | null; model: string; effort: string };

export function effortFor(model: ProviderModel, preferred = "Medium") {
  return (
    model.efforts.find((effort) => effort.toLowerCase() === preferred.toLowerCase()) ??
    model.efforts.find((effort) => effort.toLowerCase() === "medium") ??
    model.efforts[0] ??
    ""
  );
}
function selectionFor(account: ProviderAccount, model: ProviderModel, preferred?: string) {
  return { account: account.id, model: model.id, effort: effortFor(model, preferred) };
}
export function defaultSelection(state: Readonly<ProvidersState>): ProviderSelection | null {
  const account = state.accounts.find((entry) => entry.available && entry.models.length > 0);
  return account ? accountDefaultSelection(account) : null;
}
export function accountDefaultSelection(
  account: ProviderAccount,
  preferred?: string,
): ProviderSelection | null {
  const { defaults } = account;
  const configuredModel = defaults?.model;
  const configuredEffort = defaults?.effort;
  const model =
    configuredModel !== undefined && configuredModel !== null
      ? account.models.find((entry) => entry.id === configuredModel)
      : account.models[0];
  if (
    !model ||
    (configuredEffort !== undefined &&
      configuredEffort !== null &&
      !model.efforts.includes(configuredEffort))
  ) {
    return null;
  }
  return selectionFor(account, model, defaults?.effort ?? preferred);
}
export function auxiliarySelection(account: ProviderAccount): ProviderSelection | null {
  const main = accountDefaultSelection(account);
  if (!main || !account.available) {
    return null;
  }
  const auxiliary = account.defaults?.auxiliaryModel;
  if (auxiliary === null || auxiliary === undefined) {
    return main;
  }
  const model = account.models.find((entry) => entry.id === auxiliary);
  return model ? selectionFor(account, model) : null;
}
export function selectAccount(
  state: Readonly<ProvidersState>,
  accountId: string,
  current: ProviderSelection,
): ProviderSelection | null {
  const account = state.accounts.find((entry) => entry.id === accountId && entry.available);
  return account ? accountDefaultSelection(account, current.effort) : null;
}
export function resolveSelection(state: Readonly<ProvidersState>, selection: ProviderSelection) {
  const account = state.accounts.find((entry) => entry.id === selection.account);
  const model = account?.models.find((entry) => entry.id === selection.model);
  return { account, model, available: account?.available === true && model !== undefined };
}
export function selectModel(
  state: Readonly<ProvidersState>,
  current: ProviderSelection,
  modelId: string,
): ProviderSelection | null {
  const { account } = resolveSelection(state, current);
  const model = account?.models.find((entry) => entry.id === modelId);
  return account?.available === true && model ? selectionFor(account, model, current.effort) : null;
}
