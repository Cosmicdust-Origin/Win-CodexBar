import { useEffect, useState } from "react";
import { useFieldAria } from "../../../components/FormControls";
import type { LocaleKey } from "../../../i18n/keys";
import {
  MAX_ROLLING_DAYS,
  PRESET_COST_PERIODS,
  costPeriodLabel,
  customPeriodRaw,
  isPresetCostPeriod,
  rollingDays,
} from "../../../lib/costPeriod";

const CUSTOM_OPTION = "custom";

/**
 * History window picker: month to date, all, 1/7/30/90/365 days, or a custom
 * day count in 1..=365. `value` is the persisted form (`rolling:N`,
 * `month-to-date`, `all`). A custom count is committed on blur or Enter, once
 * it is valid, so typing a multi-digit value triggers one rescan.
 *
 * Renders inside a `Field` (Settings > Menu > Cost summary), which names the
 * picker.
 */
export default function CostPeriodControl({
  value,
  onChange,
  disabled = false,
  t,
}: {
  value: string;
  onChange: (raw: string) => void;
  disabled?: boolean;
  t: (key: LocaleKey) => string;
}) {
  const [customMode, setCustomMode] = useState(() => !isPresetCostPeriod(value));
  const [customText, setCustomText] = useState(() => String(rollingDays(value) ?? ""));
  // Follow external changes (settings loaded after mount, another window).
  useEffect(() => {
    if (isPresetCostPeriod(value)) {
      setCustomMode(false);
    } else {
      setCustomMode(true);
      setCustomText(String(rollingDays(value) ?? ""));
    }
  }, [value]);

  const fieldAria = useFieldAria();
  const customInvalid = customMode && customText !== "" && customPeriodRaw(customText) === null;
  const commitCustom = () => {
    const raw = customPeriodRaw(customText);
    if (raw && raw !== value) onChange(raw);
  };

  return (
    <div className="cost-period-control">
      <select
        className="select select--menu"
        value={customMode ? CUSTOM_OPTION : value}
        disabled={disabled}
        {...fieldAria}
        onChange={(event) => {
          const next = event.target.value;
          if (next === CUSTOM_OPTION) {
            // Nothing is saved until a valid count is typed.
            setCustomMode(true);
            setCustomText(String(rollingDays(value) ?? ""));
            return;
          }
          setCustomMode(false);
          onChange(next);
        }}
      >
        {PRESET_COST_PERIODS.map((raw) => (
          <option key={raw} value={raw}>
            {costPeriodLabel(raw, t)}
          </option>
        ))}
        <option value={CUSTOM_OPTION}>{t("CostPeriodCustom")}</option>
      </select>
      {customMode && (
        <input
          type="number"
          className="number-input"
          inputMode="numeric"
          min={1}
          max={MAX_ROLLING_DAYS}
          step={1}
          value={customText}
          disabled={disabled}
          aria-label={t("CostPeriodCustomDays")}
          aria-invalid={customInvalid}
          onChange={(event) => setCustomText(event.target.value)}
          onBlur={commitCustom}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              commitCustom();
            }
          }}
        />
      )}
    </div>
  );
}
