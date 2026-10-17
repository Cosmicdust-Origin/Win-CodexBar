/** Mirrors `MERGED_OVERVIEW_PROVIDER_LIMIT` in `rust/src/settings/types.rs` (macOS limit). */
export const MERGED_OVERVIEW_PROVIDER_LIMIT = 6;

/**
 * Providers shown on the tray panel's Overview, in `ordered` order: the saved
 * selection when any of it is still present, otherwise the first six
 * (macOS `mergedOverviewSelectedProviders`; empty means "first six").
 */
export function selectOverviewProviders<T extends { providerId: string }>(
  ordered: readonly T[],
  selection: readonly string[],
): T[] {
  const chosen = new Set(selection);
  const picked = ordered.filter((provider) => chosen.has(provider.providerId));
  return (picked.length > 0 ? picked : ordered).slice(0, MERGED_OVERVIEW_PROVIDER_LIMIT);
}

/** The ids the Overview shows today, for the Settings summary and dialog. */
export function effectiveOverviewSelection(
  enabledInOrder: readonly string[],
  selection: readonly string[],
): string[] {
  return selectOverviewProviders(
    enabledInOrder.map((providerId) => ({ providerId })),
    selection,
  ).map((provider) => provider.providerId);
}

/**
 * The selection after checking or unchecking one provider in the Configure
 * sheet. Starts from what Overview shows today, keeps provider order, and
 * never exceeds the limit. Unchecking the last one saves an empty list,
 * which means the default first six again.
 */
export function toggleOverviewProvider(
  enabledInOrder: readonly string[],
  selection: readonly string[],
  providerId: string,
  checked: boolean,
): string[] {
  const current = new Set(effectiveOverviewSelection(enabledInOrder, selection));
  if (checked) {
    if (current.size >= MERGED_OVERVIEW_PROVIDER_LIMIT) return [...current];
    current.add(providerId);
  } else {
    current.delete(providerId);
  }
  return enabledInOrder.filter((id) => current.has(id));
}
