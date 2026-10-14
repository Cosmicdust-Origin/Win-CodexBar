import type {
  CostSummaryDisplayStyle,
  ProviderChartData,
  ProviderUsageSnapshot,
} from "../../types/bridge";
import { formatCostAmount } from "../../lib/cardCost";
import type { MetricEntry } from "./CardMetricRow";

export interface MenuCardPresence {
  hasMetrics: boolean;
  /** Inventory rows or provider key-value details. */
  hasDetailSections: boolean;
  /**
   * Draw the detail sections: always on the full card, and on the compact
   * Overview only when it has no metric rows (upstream
   * `showsOverviewSupplementalContent`: `!compact || metrics.isEmpty`).
   */
  showsDetailSections: boolean;
  hasCost: boolean;
  localUsage: ProviderChartData["localUsage"] | null;
  wayfinderUsage: ProviderUsageSnapshot["wayfinderUsage"] | null;
  hasDetails: boolean;
}

/**
 * A cost that carries only a balance (no cap, not a spend summary) adds
 * nothing when a provider detail row already shows that balance. Upstream
 * has no cost block for such providers, so the card keeps the detail row.
 */
function balanceRepeatsDetail(provider: ProviderUsageSnapshot): boolean {
  const cost = provider.cost;
  // A provider that styles its cost block (upstream `ProviderCostMenuCardStyle`,
  // such as LithosAI's "Credits") keeps it beside the matching detail row.
  if (cost?.titleKey) return false;
  if (!cost || cost.balance == null || (cost.limit ?? 0) > 0 || cost.alwaysVisible === true) {
    return false;
  }
  const balance = cost.formattedBalance || formatCostAmount(cost.balance, cost.currencyCode);
  return (provider.displayDetails ?? []).some((detail) => detail.value.trim() === balance);
}

/**
 * Which card sections exist. A card with an error, or whose only content the
 * compact Overview drops, renders its header alone with no divider.
 */
export function describeCard(
  provider: ProviderUsageSnapshot,
  chartData: ProviderChartData | null,
  visibleMetrics: MetricEntry[],
  costSummaryDisplayStyle: CostSummaryDisplayStyle = "detailed",
  compactOverview = false,
): MenuCardPresence {
  const hasError = !!provider.error;
  const hasMetrics = !hasError && visibleMetrics.length > 0;
  const hasCost =
    !hasError &&
    !!provider.cost &&
    (costSummaryDisplayStyle !== "hidden" || provider.cost.alwaysVisible === true) &&
    !balanceRepeatsDetail(provider);
  const hasDetailSections =
    !hasError &&
    ((provider.inventory?.length ?? 0) > 0 || (provider.displayDetails?.length ?? 0) > 0);
  const showsDetailSections = hasDetailSections && (!compactOverview || !hasMetrics);
  const localUsage = hasError ? null : chartData?.localUsage ?? null;
  const wayfinderUsage =
    !hasError && provider.providerId === "wayfinder" ? provider.wayfinderUsage ?? null : null;
  const hasDetails = compactOverview
    ? hasMetrics || showsDetailSections || !!wayfinderUsage
    : hasMetrics || hasDetailSections || hasCost || !!localUsage || !!wayfinderUsage;
  return {
    hasMetrics,
    hasDetailSections,
    showsDetailSections,
    hasCost,
    localUsage,
    wayfinderUsage,
    hasDetails,
  };
}
