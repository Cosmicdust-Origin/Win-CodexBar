import type {
  QuotaWarningWindow,
  SettingsSnapshot,
  SettingsUpdate,
} from "../../../../types/bridge";
import type { LocaleKey } from "../../../../i18n/keys";
import { NumberInput, SettingsGroup } from "../../../../components/FormControls";
import {
  QUOTA_WARNING_WINDOWS,
  quotaWarningMode,
  quotaWarningWindow,
  type QuotaWarningMode,
  type QuotaWarningWindowId,
} from "../../../../lib/quotaWarnings";

type QuotaSettings = Pick<
  SettingsSnapshot,
  | "highUsageThreshold"
  | "criticalUsageThreshold"
  | "providerQuotaWarnings"
  | "showNotifications"
  | "quotaWarningMarkersVisible"
>;

interface Props {
  providerId: string;
  settings: QuotaSettings;
  disabled: boolean;
  t: (key: LocaleKey) => string;
  onChange: (patch: SettingsUpdate) => void;
}

const WINDOW_LABEL: Record<QuotaWarningWindowId, LocaleKey> = {
  session: "ProviderSessionLabel",
  weekly: "ProviderWeeklyLabel",
};

const MODES: { mode: QuotaWarningMode; label: LocaleKey }[] = [
  { mode: "global", label: "QuotaWarningGlobal" },
  { mode: "custom", label: "QuotaWarningCustom" },
  { mode: "off", label: "QuotaWarningOff" },
];

/**
 * Per-provider quota warnings (macOS `ProviderQuotaWarningSettingsView`):
 * Session and Weekly rows, each Global, Custom or Off. Values are *used* %,
 * like the global thresholds on the Notifications tab. Dimmed and locked while
 * both quota notifications and usage-bar markers are off.
 */
export function QuotaWarningsSection({
  providerId,
  settings,
  disabled,
  t,
  onChange,
}: Props) {
  const controlsEnabled =
    settings.showNotifications || settings.quotaWarningMarkersVisible;
  const footer = settings.showNotifications
    ? t("QuotaWarningProviderInherits")
    : settings.quotaWarningMarkersVisible
      ? t("QuotaWarningProviderMarkersOnly")
      : t("QuotaWarningProviderDisabled");
  const locked = disabled || !controlsEnabled;

  // A provider patch replaces that provider's whole value, so keep the other window.
  const write = (window: QuotaWarningWindowId, next: QuotaWarningWindow) => {
    const current = settings.providerQuotaWarnings?.[providerId] ?? {};
    onChange({
      providerQuotaWarnings: { [providerId]: { ...current, [window]: next } },
    });
  };

  const setMode = (window: QuotaWarningWindowId, mode: QuotaWarningMode) => {
    const stored = quotaWarningWindow(settings, providerId, window);
    const current = quotaWarningMode(stored);
    if (mode === current) return;
    if (mode === "global") {
      write(window, {});
    } else if (mode === "custom") {
      write(window, {
        enabled: true,
        high: stored.high ?? settings.highUsageThreshold,
        critical: stored.critical ?? settings.criticalUsageThreshold,
      });
    } else {
      // Off keeps custom values so switching back to Custom restores them.
      write(
        window,
        current === "custom"
          ? { enabled: false, high: stored.high, critical: stored.critical }
          : { enabled: false },
      );
    }
  };

  const setThreshold = (
    window: QuotaWarningWindowId,
    field: "high" | "critical",
    value: number,
  ) => {
    const stored = quotaWarningWindow(settings, providerId, window);
    write(window, {
      enabled: true,
      high: stored.high ?? settings.highUsageThreshold,
      critical: stored.critical ?? settings.criticalUsageThreshold,
      [field]: Math.min(100, Math.max(0, Math.round(value))),
    });
  };

  return (
    <SettingsGroup
      className={`provider-detail-group provider-quota-warnings${controlsEnabled ? "" : " is-dimmed"}`}
      title={t("QuotaWarningsTitle")}
      footer={<p>{footer}</p>}
    >
      {QUOTA_WARNING_WINDOWS.map((window) => {
        const stored = quotaWarningWindow(settings, providerId, window);
        const mode = quotaWarningMode(stored);
        const label = t(WINDOW_LABEL[window]);
        return (
          <div className="quota-warning-row" key={window} data-window={window}>
            <span className="quota-warning-row__label">{label}</span>
            <div className="provider-detail-segmented quota-warning-row__modes" role="radiogroup" aria-label={label}>
              {MODES.map(({ mode: option, label: optionLabel }) => (
                <button
                  key={option}
                  type="button"
                  role="radio"
                  aria-checked={mode === option}
                  className={`provider-detail-segmented__option${mode === option ? " is-active" : ""}`}
                  disabled={locked}
                  onClick={() => setMode(window, option)}
                >
                  {t(optionLabel)}
                </button>
              ))}
            </div>
            <div className="quota-warning-row__detail">
              {mode === "global" && (
                <span className="quota-warning-row__inherited">
                  {t("QuotaWarningInherited")
                    .replace("{}", String(settings.highUsageThreshold))
                    .replace("{}", String(settings.criticalUsageThreshold))}
                </span>
              )}
              {mode === "custom" && (
                <>
                  <label className="quota-warning-field">
                    <span>{t("QuotaWarningWarning")}</span>
                    <NumberInput
                      value={stored.high ?? settings.highUsageThreshold}
                      min={0}
                      max={100}
                      step={5}
                      disabled={locked}
                      ariaLabel={`${label} ${t("QuotaWarningWarning")}`}
                      onChange={(v) => setThreshold(window, "high", v)}
                    />
                    <span aria-hidden>%</span>
                  </label>
                  <label className="quota-warning-field">
                    <span>{t("QuotaWarningCritical")}</span>
                    <NumberInput
                      value={stored.critical ?? settings.criticalUsageThreshold}
                      min={0}
                      max={100}
                      step={5}
                      disabled={locked}
                      ariaLabel={`${label} ${t("QuotaWarningCritical")}`}
                      onChange={(v) => setThreshold(window, "critical", v)}
                    />
                    <span aria-hidden>%</span>
                  </label>
                </>
              )}
            </div>
          </div>
        );
      })}
    </SettingsGroup>
  );
}
