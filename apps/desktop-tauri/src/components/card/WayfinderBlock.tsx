import type { ProviderUsageSnapshot } from "../../types/bridge";
import { useLocale } from "../../hooks/useLocale";

const compact = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 });

function compactCount(value: number | null | undefined): string {
  if (value == null || value <= 0) return "—";
  return compact.format(value);
}

/** Wayfinder gateway telemetry, laid out like the inline dashboard KPIs. */
export default function WayfinderBlock({
  usage,
}: {
  usage: NonNullable<ProviderUsageSnapshot["wayfinderUsage"]>;
}) {
  const { t } = useLocale();
  const amount = (value: number) =>
    usage.priced ? `${value.toFixed(4)} ${usage.unit.toUpperCase()}` : "—";
  const kpis: [string, string][] = [
    [t("WayfinderGatewayStatus"), usage.gatewayStatus],
    [t("WayfinderModels"), String(usage.modelCount)],
    [t("WayfinderRequests"), compactCount(usage.requests)],
    [t("WayfinderTokens"), compactCount(usage.tokens)],
  ];
  const notes = [
    usage.offline ? t("WayfinderOffline") : null,
    usage.dryRun ? t("WayfinderDryRun") : null,
    usage.missingKeys.length > 0
      ? `${t("WayfinderMissingKeys")}: ${usage.missingKeys.join(", ")}`
      : null,
  ].filter((note): note is string => note != null);

  return (
    <section className="card-dash card-wayfinder">
      <div className="card-dash__kpis">
        {kpis.map(([title, value]) => (
          <div className="card-dash__kpi" key={title}>
            <span className="card-dash__kpi-title">{title}</span>
            <span className="card-dash__kpi-value">{value}</span>
          </div>
        ))}
      </div>
      <span className="card-dash__window-title">
        {t("WayfinderSaved")}: {amount(usage.saved)} ({usage.savedPercent.toFixed(1)}%)
      </span>
      {notes.length > 0 && (
        <div className="card-dash__details">
          {notes.map((note) => (
            <span className="card-dash__detail" key={note}>
              {note}
            </span>
          ))}
        </div>
      )}
    </section>
  );
}
