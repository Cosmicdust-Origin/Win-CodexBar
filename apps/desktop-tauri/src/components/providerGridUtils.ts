import type { ProviderUsageSnapshot } from "../types/bridge";

export function prioritizeProviders(
  providers: ProviderUsageSnapshot[],
  selectedProviderId: string | null,
): ProviderUsageSnapshot[] {
  if (!selectedProviderId) return providers;
  const selectedIndex = providers.findIndex((provider) => provider.providerId === selectedProviderId);
  if (selectedIndex < 0 || selectedIndex < 18) return providers;
  const selected = providers[selectedIndex];
  return [selected, ...providers.slice(0, selectedIndex), ...providers.slice(selectedIndex + 1)];
}

const WEEK_MINUTES = 7 * 24 * 60;

/**
 * The lane the switcher's progress track shows. Like macOS
 * `UsageSnapshot.switcherWeeklyWindow`, it prefers a real 7-day lane (the
 * most used one) because the row is labelled "Weekly progress", and falls back
 * to the primary lane. Only the core primary and secondary lanes are read:
 * tertiary and extra lanes are model-scoped carve-outs on some providers
 * (macOS drops Claude's for the same reason), so they stay on the card.
 */
export function switcherWeeklyPercent(provider: ProviderUsageSnapshot, showAsUsed: boolean): number {
  type Lane = ProviderUsageSnapshot["primary"];
  const weekly = [provider.primary, provider.secondary].filter(
    (window): window is Lane => window?.windowMinutes === WEEK_MINUTES,
  );
  const lane = weekly.reduce<Lane | null>(
    (most, window) => (most === null || window.usedPercent > most.usedPercent ? window : most),
    null,
  ) ?? provider.primary;
  const pct = showAsUsed ? lane.usedPercent : lane.remainingPercent;
  return Math.max(0, Math.min(100, pct));
}
