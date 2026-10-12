import type { BootstrapState, SettingsTabId, SettingsUpdate } from "../../types/bridge";
import type { LocaleKey } from "../../i18n/keys";

export const TAB_META: { id: SettingsTabId; labelKey: LocaleKey }[] = [
  { id: "general", labelKey: "TabGeneral" },
  { id: "providers", labelKey: "TabProviders" },
  { id: "notifications", labelKey: "TabNotifications" },
  { id: "menuBar", labelKey: "TabMenuBar" },
  { id: "menu", labelKey: "TabMenu" },
  { id: "usageSpend", labelKey: "TabUsageSpend" },
  { id: "advanced", labelKey: "TabAdvanced" },
  { id: "hooks", labelKey: "TabHooks" },
  { id: "about", labelKey: "TabAbout" },
];

export interface TabProps {
  settings: BootstrapState["settings"];
  set: (p: SettingsUpdate) => void;
  saving: boolean;
}

export function isSettingsTab(value: string): value is SettingsTabId {
  return TAB_META.some((t) => t.id === value);
}

/** A Settings window request: a tab plus, on Providers, the provider to select. */
export interface SettingsTabRequest {
  tab: SettingsTabId;
  providerId: string | null;
}

/**
 * Parse a raw `open_settings_window` tab argument. `providers:<id>` selects a
 * provider on the Providers tab; the tab id still has to be one of `TAB_META`.
 * Unknown tabs return null so callers keep their own fallback.
 */
export function parseSettingsTabRequest(raw: string | null | undefined): SettingsTabRequest | null {
  if (!raw) return null;
  const separator = raw.indexOf(":");
  const tab = separator < 0 ? raw : raw.slice(0, separator);
  if (!isSettingsTab(tab)) return null;
  const providerId = separator < 0 ? "" : raw.slice(separator + 1).trim();
  return { tab, providerId: tab === "providers" && providerId ? providerId : null };
}

/** The `open_settings_window` argument that opens Settings > Providers on one provider. */
export function providersTabRequest(providerId: string): `providers:${string}` {
  return `providers:${providerId}`;
}

/** macOS system colors behind the white glyph of a sidebar pane chip. */
export type SidebarChipColor = "gray" | "green" | "red" | "blue" | "teal" | "purple" | "orange";

export interface SidebarPane {
  id: Exclude<SettingsTabId, "providers">;
  labelKey: LocaleKey;
  /** `null` shows the app icon instead of a chip (About). */
  chip: SidebarChipColor | null;
}

/**
 * App panes in the Settings sidebar, in the macOS 0.70 order
 * (`PreferencesSidebar.swift` appPanesSection). iCloud Sync, Plugins and Debug
 * have no Windows pane.
 */
export const SIDEBAR_PANES: SidebarPane[] = [
  { id: "general", labelKey: "TabGeneral", chip: "gray" },
  { id: "usageSpend", labelKey: "TabUsageSpend", chip: "green" },
  { id: "notifications", labelKey: "TabNotifications", chip: "red" },
  { id: "menuBar", labelKey: "TabMenuBar", chip: "blue" },
  { id: "menu", labelKey: "TabMenu", chip: "teal" },
  { id: "advanced", labelKey: "TabAdvanced", chip: "purple" },
  { id: "hooks", labelKey: "TabHooks", chip: "orange" },
  { id: "about", labelKey: "TabAbout", chip: null },
];

interface NamedProvider {
  id: string;
  displayName: string;
}

/** Case-insensitive match on display name or id; a blank query keeps every provider. */
export function filterProvidersByQuery<T extends NamedProvider>(providers: T[], query: string): T[] {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return providers;
  return providers.filter(
    (provider) =>
      provider.displayName.toLocaleLowerCase().includes(needle) ||
      provider.id.toLocaleLowerCase().includes(needle),
  );
}

/**
 * Enabled providers first, then by display name ignoring case, then by id
 * (macOS `CodexBarConfig.alphabeticalProviderOrder`). Display only: the saved
 * provider order is never rewritten.
 */
export function sortProvidersAlphabetically<T extends NamedProvider>(
  providers: T[],
  enabled: ReadonlySet<string>,
): T[] {
  return [...providers].sort((lhs, rhs) => {
    const lhsOn = enabled.has(lhs.id);
    if (lhsOn !== enabled.has(rhs.id)) return lhsOn ? -1 : 1;
    const byName = lhs.displayName.localeCompare(rhs.displayName, undefined, {
      sensitivity: "base",
    });
    if (byName !== 0) return byName;
    return lhs.id < rhs.id ? -1 : lhs.id > rhs.id ? 1 : 0;
  });
}

/** Reordering is only meaningful in the saved order with nothing filtered out (macOS). */
export function canReorderProviders(sortedAlphabetically: boolean, query: string): boolean {
  return !sortedAlphabetically && query.trim() === "";
}

/**
 * The enabled provider ids after switching `id` on or off, in catalog order.
 * Shared by the sidebar row switches and the provider header switch.
 */
export function nextEnabledProviders(
  catalog: readonly { id: string }[],
  enabled: Iterable<string>,
  id: string,
  on: boolean,
): string[] {
  const next = new Set(enabled);
  if (on) next.add(id);
  else next.delete(id);
  return catalog.filter((p) => next.has(p.id)).map((p) => p.id);
}
