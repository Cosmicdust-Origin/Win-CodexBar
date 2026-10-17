import { useLocale } from "../hooks/useLocale";
import { localizeProviderText } from "../lib/providerText";
import { localizeProviderLabel } from "../lib/windowLabels";
import type { ProviderDisplayDetail } from "../types/bridge";

const MAX_ROWS_PER_SECTION = 24;

export interface ProviderDisplayDetailGroup {
  id: number;
  title: string | null;
  rows: ProviderDisplayDetail[];
}

/** Group consecutive provider details and cap each rendered section. */
export function groupProviderDisplayDetails(
  details: ProviderDisplayDetail[],
): ProviderDisplayDetailGroup[] {
  const groups: ProviderDisplayDetailGroup[] = [];
  for (const detail of details) {
    const title = detail.sectionTitle ?? null;
    const current = groups[groups.length - 1];
    if (
      current &&
      current.title === title &&
      current.rows.length < MAX_ROWS_PER_SECTION
    ) {
      current.rows.push(detail);
    } else {
      groups.push({ id: groups.length, title, rows: [detail] });
    }
  }
  return groups;
}

/**
 * One transient provider detail line: "{title}: {value} [secondary]"
 * plus an optional clamped progress bar.
 *
 * Used by the settings provider detail (`UsageSection`); the tray card renders
 * the same data as key-value rows in `card/DetailSections`.
 */
export function ProviderDisplayRow({
  detail,
  lineClassName,
  secondaryClassName,
  trackClassName,
  fillClassName,
}: {
  detail: ProviderDisplayDetail;
  lineClassName: string;
  secondaryClassName?: string;
  trackClassName: string;
  fillClassName: string;
}) {
  const { t } = useLocale();
  const title = localizeProviderLabel(detail.title, t);
  const progress = detail.progress;
  const progressPercent =
    progress &&
    Number.isFinite(progress.used) &&
    Number.isFinite(progress.total) &&
    progress.total > 0
      ? Math.max(0, Math.min(100, (progress.used / progress.total) * 100))
      : null;

  return (
    <div>
      <div className={lineClassName}>
        <span>{title}: {localizeProviderText(detail.value, t)}</span>
        {detail.secondaryValue && secondaryClassName && (
          <>
            {" "}
            <span className={secondaryClassName}>
              {localizeProviderText(detail.secondaryValue, t)}
            </span>
          </>
        )}
      </div>
      {progressPercent != null && (
        <div
          className={trackClassName}
          role="progressbar"
          aria-label={title}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={progressPercent}
        >
          <div className={fillClassName} style={{ width: `${progressPercent}%` }} />
        </div>
      )}
    </div>
  );
}
