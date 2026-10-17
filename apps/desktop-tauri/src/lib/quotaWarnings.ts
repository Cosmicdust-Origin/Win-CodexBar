import type { QuotaWarningWindow, SettingsSnapshot } from "../types/bridge";

/** The two windows that carry quota warnings (Mac `QuotaWarningWindow`). */
export type QuotaWarningWindowId = "session" | "weekly";

export const QUOTA_WARNING_WINDOWS: readonly QuotaWarningWindowId[] = ["session", "weekly"];

/** Mac `QuotaWarningProviderMode`. */
export type QuotaWarningMode = "global" | "custom" | "off";

export type QuotaWarningSettings = Pick<
  SettingsSnapshot,
  "highUsageThreshold" | "criticalUsageThreshold" | "providerQuotaWarnings"
>;

export interface ResolvedQuotaWarning {
  mode: QuotaWarningMode;
  /** False only when the window is Off for this provider. */
  enabled: boolean;
  /** *Used* percentages. */
  high: number;
  critical: number;
}

/** The stored window for a provider; `{}` when it follows the global settings. */
export function quotaWarningWindow(
  settings: Pick<SettingsSnapshot, "providerQuotaWarnings">,
  providerId: string,
  window: QuotaWarningWindowId,
): QuotaWarningWindow {
  return settings.providerQuotaWarnings?.[providerId]?.[window] ?? {};
}

export function quotaWarningMode(value: QuotaWarningWindow): QuotaWarningMode {
  if (value.enabled == null && value.high == null && value.critical == null) return "global";
  return value.enabled === false ? "off" : "custom";
}

/** Thresholds and on/off state for one provider window, mirroring `Settings::usage_thresholds`. */
export function resolveQuotaWarning(
  settings: QuotaWarningSettings,
  providerId: string,
  window: QuotaWarningWindowId,
): ResolvedQuotaWarning {
  const stored = quotaWarningWindow(settings, providerId, window);
  return {
    mode: quotaWarningMode(stored),
    enabled: stored.enabled !== false,
    high: stored.high ?? settings.highUsageThreshold,
    critical: stored.critical ?? settings.criticalUsageThreshold,
  };
}
