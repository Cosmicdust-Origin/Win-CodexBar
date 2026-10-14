import type { ProviderDetail } from "../../../../types/bridge";
import type { LocaleKey } from "../../../../i18n/keys";
import { ProviderIcon } from "../../../../components/providers/ProviderIcon";
import { Toggle } from "../../../../components/FormControls";
import { hideOpenAiApiProjectId } from "../../../../lib/openAiApiIdentity";
import { providerSourceHintShort } from "../providerDetailFormat";

interface Props {
  provider: Pick<
    ProviderDetail,
    "id" | "displayName" | "email" | "organization" | "plan" | "authType" | "sourceLabel"
  >;
  subtitle: string;
  t: (key: LocaleKey) => string;
  hidePersonalInfo?: boolean;
  /** "Updated" row value; the row is omitted when absent. */
  updatedText?: string | null;
  /** Header switch bound to the enabled-providers list; omitted when absent. */
  enabled?: boolean;
  onToggleEnabled?: (on: boolean) => void;
  onRefresh?: () => void;
  busy?: boolean;
}

/**
 * Header group of the provider pane (macOS `ProviderDetailHeaderRow` +
 * `ProviderDetailInfoRows`): icon, name and subtitle, a borderless Refresh
 * button and the Enabled switch, then Source / Updated / Account / Auth / Plan.
 */
export function IdentitySection({
  provider,
  subtitle,
  t,
  hidePersonalInfo = false,
  updatedText = null,
  enabled,
  onToggleEnabled,
  onRefresh,
  busy = false,
}: Props) {
  const hideProjectId = provider.id === "openaiapi" && hidePersonalInfo;
  // Hide Personal Info drops the account row for every provider, as the tray
  // card (MenuCard `headerAccount`) and the Mac Account row do.
  const account = hidePersonalInfo ? null : provider.email || provider.organization;
  const rows: { key: string; label: string; value: string | null }[] = [
    {
      key: "source",
      label: t("Source"),
      value: provider.sourceLabel || providerSourceHintShort(provider.id, t),
    },
    { key: "updated", label: t("Updated"), value: updatedText },
    {
      key: "account",
      label: t("Account"),
      value: hideOpenAiApiProjectId(account, hideProjectId),
    },
    { key: "auth", label: t("AuthType"), value: provider.authType },
    {
      key: "plan",
      label: t("Plan"),
      value: hideOpenAiApiProjectId(displayIdentityValue(provider.plan, t), hideProjectId),
    },
  ];
  const visible = rows.filter(
    (r): r is { key: string; label: string; value: string } =>
      !!r.value && r.value.length > 0,
  );

  return (
    <section className="settings-group provider-detail-group provider-detail-header-group">
      <div className="settings-group__box">
        <div className="provider-detail-header">
          <ProviderIcon providerId={provider.id} size={28} />
          <div className="provider-detail-title-group">
            <div className="provider-detail-title">{provider.displayName}</div>
            {subtitle && <div className="provider-detail-subtitle">{subtitle}</div>}
          </div>
          {onRefresh && (
            <button
              type="button"
              className="provider-detail-icon-btn"
              onClick={onRefresh}
              disabled={busy}
              aria-label={t("ActionRefresh")}
              title={t("ActionRefresh")}
            >
              <svg viewBox="0 0 16 16" width={14} height={14} aria-hidden fill="none">
                <path
                  d="M13 8a5 5 0 1 1-1.46-3.54M13 3v2.5h-2.5"
                  stroke="currentColor"
                  strokeWidth={1.5}
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
            </button>
          )}
          {onToggleEnabled && (
            <Toggle
              variant="switch"
              checked={enabled ?? false}
              ariaLabel={t("ProviderEnabled")}
              onChange={onToggleEnabled}
            />
          )}
        </div>
        {visible.map((r) => (
          <div key={r.key} className="provider-info-row">
            <span className="provider-info-row__label">{r.label}</span>
            <span className="provider-info-row__value">{r.value}</span>
          </div>
        ))}
      </div>
    </section>
  );
}

function displayIdentityValue(
  value: string | null,
  t: (key: LocaleKey) => string,
): string | null {
  if (!value) return null;
  if (value.trim().toLowerCase() === "default_claude_ai")
    return t("ProviderPlanClaudeAi");
  return value;
}
