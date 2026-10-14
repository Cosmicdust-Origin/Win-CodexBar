import { useState } from "react";
import type { LocaleKey } from "../../../../i18n/keys";
import { setProviderAutoResumeAfterQuotaReset } from "../../../../lib/tauri";
import { Field, Toggle } from "../../../../components/FormControls";

interface Props {
  providerId: string;
  enabled: boolean;
  available: boolean;
  disabled: boolean;
  t: (key: LocaleKey) => string;
  onChanged: () => void;
}

/**
 * Opt-in switch for reopening the exact local Codex or Claude CLI session.
 * Windows-only; renders as a row of the provider's Options group.
 */
export function AutoResumeSection({
  providerId,
  enabled,
  available,
  disabled,
  t,
  onChanged,
}: Props) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!available) return null;

  const handleChange = async (next: boolean) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await setProviderAutoResumeAfterQuotaReset(providerId, next);
      onChanged();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Field
        label={t("ProviderAutoResumeAfterQuotaReset")}
        description={t("ProviderAutoResumeAfterQuotaResetHelper")}
      >
        <Toggle
          variant="switch"
          checked={enabled}
          disabled={disabled || busy}
          onChange={(next) => void handleChange(next)}
        />
      </Field>
      {error && <p className="provider-option-error">{error}</p>}
    </>
  );
}
