import { useEffect, useState } from "react";
import type { LocaleKey } from "../../../../i18n/keys";
import type { SettingsUpdate } from "../../../../types/bridge";
import { getProviderIcon } from "../../../../components/providers/providerIcons";
import { SettingsGroup } from "../../../../components/FormControls";

interface Props {
  providerId: string;
  accentColor: string | null;
  disabled?: boolean;
  t: (key: LocaleKey) => string;
  onChange: (patch: SettingsUpdate) => void;
}

/**
 * Accent color group (macOS `ProviderAccentColorSection`): a hex field, the
 * native color well and "Use default", which appears only while an override
 * is saved. The override is persisted through the settings-update flow.
 */
export function AccentColorSection({
  providerId,
  accentColor,
  disabled = false,
  t,
  onChange,
}: Props) {
  const [input, setInput] = useState(accentColor ?? "");
  const [error, setError] = useState<string | null>(null);
  // Follow saves made elsewhere (another window, or the reset below).
  useEffect(() => setInput(accentColor ?? ""), [accentColor]);

  const brandColor = getProviderIcon(providerId).brandColor;
  const effective = accentColor ?? brandColor;

  const handleSave = (raw: string) => {
    setError(null);
    const trimmed = raw.trim();
    if (trimmed === "") {
      if (accentColor !== null) onChange({ providerAccentColors: { [providerId]: null } });
      setInput("");
      return;
    }
    const trimmedHex = trimmed.startsWith("#") ? trimmed.slice(1) : trimmed;
    if (trimmedHex.length !== 6 || !/^[0-9A-Fa-f]{6}$/.test(trimmedHex)) {
      setError(t("ProviderAccentColorInvalid"));
      return;
    }
    const normalized = `#${trimmedHex.toUpperCase()}`;
    setInput(normalized);
    if (normalized !== accentColor) {
      onChange({ providerAccentColors: { [providerId]: normalized } });
    }
  };

  const handleReset = () => {
    setError(null);
    onChange({ providerAccentColors: { [providerId]: null } });
    setInput("");
  };

  return (
    <SettingsGroup
      className="provider-detail-group provider-accent-color-group"
      title={t("ProviderAccentColor")}
      footer={t("ProviderAccentColorHelper")}
    >
      <div className="accent-color-row">
        <input
          type="text"
          className="text-input accent-color-input"
          value={input}
          placeholder={brandColor}
          maxLength={7}
          spellCheck={false}
          disabled={disabled}
          aria-label={`${t("ProviderAccentColor")} hex`}
          onChange={(e) => setInput(e.target.value)}
          onBlur={() => handleSave(input)}
          onKeyDown={(e) => {
            if (e.key === "Enter") handleSave(input);
          }}
        />
        <input
          type="color"
          className="accent-color-picker"
          value={effective}
          disabled={disabled}
          aria-label={t("ProviderAccentColor")}
          onChange={(e) => handleSave(e.target.value)}
        />
        {accentColor !== null && (
          <button
            type="button"
            className="provider-detail-link-btn"
            disabled={disabled}
            onClick={handleReset}
          >
            {t("ProviderAccentColorReset")}
          </button>
        )}
      </div>
      {error && (
        <p className="provider-option-error" role="alert">
          {error}
        </p>
      )}
    </SettingsGroup>
  );
}
