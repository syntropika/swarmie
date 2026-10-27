import type { ProviderAccount, ProviderModel, ProvidersState } from "./index";

/** Portable execution selection; no Conversation or Composer types are required. */
export type ProviderSelection = { account: string | null; model: string; effort: string };

function effortFor(model: ProviderModel, preferred = "Medium") {
  return model.efforts.includes(preferred)
    ? preferred
    : (model.efforts.find((effort) => effort === "Medium") ?? model.efforts[0] ?? "");
}
function selectionFor(account: ProviderAccount, model: ProviderModel, preferred?: string) {
  return { account: account.id, model: model.id, effort: effortFor(model, preferred) };
}
export function defaultSelection(state: Readonly<ProvidersState>): ProviderSelection | null {
  const account = state.accounts.find((entry) => entry.available && entry.models.length > 0);
  const model = account?.models[0];
  return account && model ? selectionFor(account, model) : null;
}
export function selectAccount(
  state: Readonly<ProvidersState>,
  accountId: string,
  current: ProviderSelection,
): ProviderSelection | null {
  const account = state.accounts.find((entry) => entry.id === accountId && entry.available);
  const model = account?.models[0];
  return account && model ? selectionFor(account, model, current.effort) : null;
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
