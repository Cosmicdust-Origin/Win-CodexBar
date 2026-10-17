import { useState } from "react";
import type { LocaleKey } from "../../../../i18n/keys";
import { setProviderOptionalDetails } from "../../../../lib/tauri";
import { Field, Toggle } from "../../../../components/FormControls";

interface Props {
  providerId: string;
  enabled: boolean;
  available: boolean;
  disabled: boolean;
  t: (key: LocaleKey) => string;
  onChanged: () => void;
}

interface Copy {
  label: LocaleKey;
  helper: LocaleKey;
}

/** Copy for the one optional breakdown each supporting provider offers. */
const OPTIONAL_DETAILS_COPY: Record<string, Copy> = {
  litellm: {
    label: "ProviderLiteLLMModelActivity",
    helper: "ProviderLiteLLMModelActivityHelper",
  },
  claude: {
    label: "ProviderClaudeWorkspaceSpend",
    helper: "ProviderClaudeWorkspaceSpendHelper",
  },
};

/**
 * Opt-in for a provider's extra breakdown (LiteLLM model activity, Claude
 * workspace spend); renders as a row of the provider's Options group.
 */
export function OptionalDetailsSection({
  providerId,
  enabled,
  available,
  disabled,
  t,
  onChanged,
}: Props) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const copy = OPTIONAL_DETAILS_COPY[providerId];
  if (!available || !copy) return null;

  const handleChange = async (next: boolean) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await setProviderOptionalDetails(providerId, next);
      onChanged();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Field label={t(copy.label)} description={t(copy.helper)}>
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
