import type { LocaleKey } from "../i18n/keys";
import type { ProviderStatusEntry, ServiceStatusIndicator } from "../types/bridge";
import { formatRelativeUpdated } from "./relativeTime";

/** macOS `ProviderStatusIndicator.label` (`UsageStoreSupport.swift:4-15`). */
export const SERVICE_STATUS_TO_KEY: Record<ServiceStatusIndicator, LocaleKey> = {
  none: "ServiceStatusOperational",
  minor: "ServiceStatusPartialOutage",
  major: "ServiceStatusMajorOutage",
  critical: "ServiceStatusCriticalIssue",
  maintenance: "ServiceStatusMaintenance",
  unknown: "ServiceStatusUnknown",
};

/**
 * The secondary menu line macOS adds under the provider actions while the
 * target provider has an incident (`MenuDescriptor.swift:611-612,645-657`):
 * the feed description, else the indicator label, then the feed's update time.
 */
export function serviceStatusLine(
  entries: readonly ProviderStatusEntry[],
  providerId: string | null,
  t: (key: LocaleKey) => string,
  nowMs: number = Date.now(),
): string | null {
  if (providerId === null) return null;
  const status = entries.find((entry) => entry.providerId === providerId)?.status;
  if (!status || status.indicator === "none") return null;
  const description = status.description?.trim();
  const label = description ? description : t(SERVICE_STATUS_TO_KEY[status.indicator]);
  if (status.updatedAt === null) return label;
  return `${label} — ${formatRelativeUpdated(status.updatedAt, t, nowMs)}`;
}
