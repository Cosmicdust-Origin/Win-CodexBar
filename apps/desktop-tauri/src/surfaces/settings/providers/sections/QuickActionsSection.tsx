import type { ProviderDetail } from "../../../../types/bridge";
import type { LocaleKey } from "../../../../i18n/keys";

interface Props {
  provider: Pick<ProviderDetail, "dashboardUrl" | "statusPageUrl" | "buyCreditsUrl">;
  busy: boolean;
  onSwitchAccount: () => void;
  onOpenDashboard: () => void;
  onOpenStatusPage: () => void;
  onBuyCredits: () => void;
  t: (key: LocaleKey) => string;
}

/**
 * Windows-only link row at the end of the Usage group. Refresh lives in the
 * header; links without a URL on the provider are omitted, and the row
 * renders nothing when no link applies.
 */
export function QuickActionsSection({
  provider,
  busy,
  onSwitchAccount,
  onOpenDashboard,
  onOpenStatusPage,
  onBuyCredits,
  t,
}: Props) {
  const links: { key: string; label: LocaleKey; onClick: () => void; disabled?: boolean }[] = [];
  if (provider.dashboardUrl) {
    links.push({ key: "switch", label: "ActionSwitchAccount", onClick: onSwitchAccount, disabled: busy });
    links.push({ key: "dashboard", label: "ActionUsageDashboard", onClick: onOpenDashboard });
  }
  if (provider.statusPageUrl) {
    links.push({ key: "status", label: "ActionStatusPage", onClick: onOpenStatusPage });
  }
  if (provider.buyCreditsUrl) {
    links.push({ key: "credits", label: "ActionBuyCredits", onClick: onBuyCredits });
  }
  if (links.length === 0) return null;

  return (
    <div className="provider-detail-links">
      {links.map((link) => (
        <button
          key={link.key}
          type="button"
          className="provider-detail-link-btn"
          disabled={link.disabled}
          onClick={link.onClick}
        >
          {t(link.label)}
        </button>
      ))}
    </div>
  );
}
