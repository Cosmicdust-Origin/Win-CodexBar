import type { LocaleKey } from "../i18n/keys";
import type { ProviderUsageSnapshot, RateWindowSnapshot } from "../types/bridge";
import { providersTabRequest } from "../surfaces/settings/settingsTabs";
import { hasSuccessfulClaudeCliQuota } from "./claudeAccountActions";

/** Providers whose account manager lives in Settings > Providers > that provider. */
const ACCOUNT_MANAGER_PROVIDERS = new Set(["codex", "claude", "grok"]);

export interface AccountMenuAction {
  labelKey: Extract<LocaleKey, "TrayMenuSwitchAccount" | "AddAccountEllipsis">;
  /** `open_settings_window` argument that selects the provider. */
  settingsTab: string;
}

/**
 * The tray footer account row (Mac `MenuDescriptor` "Switch Account..." or
 * "Add Account..."). It replaces the account menus that used to sit inside
 * the card, and deep-links to the provider's existing Settings account UI.
 */
export function accountMenuAction(
  provider: ProviderUsageSnapshot | null,
): AccountMenuAction | null {
  if (!provider || !ACCOUNT_MANAGER_PROVIDERS.has(provider.providerId)) return null;
  // Upstream `CodexProviderImplementation.loginMenuAction` always offers
  // "Add Account..." for Codex, signed in or not.
  const hasAccount =
    provider.providerId !== "codex" &&
    (Boolean(provider.accountEmail?.trim()) ||
      hasLiveClaudeIdentity(provider) ||
      hasSuccessfulClaudeCliQuota(provider));
  return {
    labelKey: hasAccount ? "TrayMenuSwitchAccount" : "AddAccountEllipsis",
    settingsTab: providersTabRequest(provider.providerId),
  };
}

function isQuotaWindow(window: RateWindowSnapshot | null): boolean {
  return window !== null && !window.isInformational && Number.isFinite(window.usedPercent);
}

/**
 * Upstream `MenuDescriptor.hasAccount`: a Claude snapshot with an identity and
 * rate-limit windows is signed in even without an email. The bridge carries
 * the identity's login method as `planName`.
 */
function hasLiveClaudeIdentity(provider: ProviderUsageSnapshot): boolean {
  return (
    provider.providerId === "claude" &&
    provider.error === null &&
    Boolean(provider.planName?.trim()) &&
    (isQuotaWindow(provider.primary) || isQuotaWindow(provider.secondary))
  );
}
