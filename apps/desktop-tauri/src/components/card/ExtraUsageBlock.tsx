import type { CostSnapshotBridge, CostSummaryDisplayStyle } from "../../types/bridge";
import { useLocale } from "../../hooks/useLocale";
import { useFormattedResetTime } from "../../hooks/useFormattedResetTime";
import { cardCostSection } from "../../lib/cardCost";

/**
 * Extra usage and cost block (upstream `MenuCardView+Costs.swift`): the title
 * in body-medium, the bar in the provider color, then the spend line on the
 * left and the percent on the right. A balance-only cost is one inline row.
 */
export default function ExtraUsageBlock({
  providerId,
  cost,
  style,
  resetTimeRelative,
}: {
  providerId: string;
  cost: CostSnapshotBridge;
  style: CostSummaryDisplayStyle | undefined;
  resetTimeRelative: boolean;
}) {
  const { t } = useLocale();
  const section = cardCostSection(cost, style, t, providerId);
  const resetText = useFormattedResetTime(
    section && !section.inline && style !== "compact" ? cost.resetsAt : null,
    null,
    resetTimeRelative,
  );
  if (!section) return null;

  const monthlySpend =
    providerId === "mistral"
      ? `${t("MistralMonthlySpend")}: ${
          cost.currencySymbol ? `${cost.currencySymbol}${cost.used.toFixed(2)}` : cost.formattedUsed
        }`
      : null;

  if (section.inline) {
    return (
      <section className="card-extra card-extra--inline">
        <div className="card-extra__line">
          <span className="card-extra__title">{section.title}</span>
          <span className="card-extra__inline-value">{section.spendLine}</span>
        </div>
        {monthlySpend && <span className="card-extra__caption">{monthlySpend}</span>}
      </section>
    );
  }

  return (
    <section className="card-extra">
      <span className="card-extra__title">{section.title}</span>
      {section.percentUsed != null && (
        <div
          className="card-extra__track"
          role="progressbar"
          aria-label={section.title}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={section.percentUsed}
        >
          <div className="card-extra__fill" style={{ width: `${section.percentUsed}%` }} />
        </div>
      )}
      <div className="card-extra__line">
        <span className="card-extra__spend">{section.spendLine}</span>
        {section.percentLine && (
          <span className="card-extra__caption">{section.percentLine}</span>
        )}
      </div>
      {section.balanceLine && <span className="card-extra__caption">{section.balanceLine}</span>}
      {resetText && (
        <span className="card-extra__caption">
          {t("DetailCostResets")}: {resetText}
        </span>
      )}
      {monthlySpend && <span className="card-extra__caption">{monthlySpend}</span>}
    </section>
  );
}
