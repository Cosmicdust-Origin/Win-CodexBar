import { useMemo, useState, type ReactElement, type ReactNode } from "react";
import type {
  BootstrapState,
  ProviderCatalogEntry,
  ProviderUsageSnapshot,
  SettingsTabId,
  SettingsUpdate,
} from "../../types/bridge";
import { useLocale } from "../../hooks/useLocale";
import { reorderProviders } from "../../lib/tauri";
import codexbarIcon from "../../assets/codexbar-icon.png";
import {
  SIDEBAR_PANES,
  canReorderProviders,
  filterProvidersByQuery,
  nextEnabledProviders,
  sortProvidersAlphabetically,
  type SidebarPane,
} from "./settingsTabs";
import {
  ProvidersSidebar,
  type ProviderSidebarRow,
  type ProviderSidebarStatus,
} from "./providers/ProvidersSidebar";

// White glyphs on the pane chips, standing in for the macOS SF Symbols.
function Glyph({ children }: { children: ReactNode }) {
  return (
    <svg
      width={14}
      height={14}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {children}
    </svg>
  );
}

const PANE_GLYPHS: Record<Exclude<SettingsTabId, "providers" | "about">, ReactElement> = {
  general: (
    <Glyph>
      <circle cx="8" cy="8" r="2" />
      <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4" />
    </Glyph>
  ),
  usageSpend: (
    <Glyph>
      <path d="M2 12.5V4.5h12v8" />
      <path d="M4.5 10V8M7.5 10V6.5M10.5 10V7.2M13 10V5.5" />
    </Glyph>
  ),
  notifications: (
    <Glyph>
      <path d="M3.5 11.5h9l-1.2-1.8V7a3.3 3.3 0 0 0-6.6 0v2.7Z" />
      <path d="M6.5 13a1.7 1.7 0 0 0 3 0" />
    </Glyph>
  ),
  menuBar: (
    <Glyph>
      <path d="M1.5 8c1.6-3 4-4.5 6.5-4.5S13 5 14.5 8c-1.5 3-4 4.5-6.5 4.5S3.1 11 1.5 8Z" />
      <circle cx="8" cy="8" r="2" />
    </Glyph>
  ),
  menu: (
    <Glyph>
      <rect x="2" y="2" width="5" height="5" rx="1" />
      <rect x="9" y="2" width="5" height="5" rx="1" />
      <rect x="2" y="9" width="5" height="5" rx="1" />
      <rect x="9" y="9" width="5" height="5" rx="1" />
    </Glyph>
  ),
  advanced: (
    <Glyph>
      <path d="M2 4h8M2 8h5M2 12h10" />
      <circle cx="11.5" cy="4" r="1.4" />
      <circle cx="8.5" cy="8" r="1.4" />
      <circle cx="13" cy="12" r="1.4" />
    </Glyph>
  ),
  // macOS `bolt.horizontal.circle.fill`.
  hooks: (
    <Glyph>
      <circle cx="8" cy="8" r="6.2" />
      <path d="M3.8 8.6 7.4 5.6 7.4 7.4H12.2L8.6 10.4 8.6 8.6Z" />
    </Glyph>
  ),
};

function PaneChip({ pane }: { pane: SidebarPane }) {
  if (pane.chip === null || pane.id === "about") {
    return <img className="settings-sidebar__app-icon" src={codexbarIcon} alt="" aria-hidden />;
  }
  return (
    <span className={`settings-sidebar__chip settings-sidebar__chip--${pane.chip}`} aria-hidden>
      {PANE_GLYPHS[pane.id]}
    </span>
  );
}

/** Fetch state of one provider for its sidebar dot. */
function deriveProviderStatus(
  isEnabled: boolean,
  snap: ProviderUsageSnapshot | null,
): ProviderSidebarStatus {
  if (!isEnabled) return "disabled";
  if (!snap) return "loading";
  if (snap.error) return "error";
  const updatedMs = new Date(snap.updatedAt).getTime();
  if (Number.isFinite(updatedMs) && (Date.now() - updatedMs) / 60_000 > 10) return "stale";
  return "ok";
}

interface SettingsSidebarProps {
  settings: BootstrapState["settings"];
  /** Provider catalog in the saved provider order. */
  providers: ProviderCatalogEntry[];
  snapshots: ProviderUsageSnapshot[];
  set: (patch: SettingsUpdate) => void;
  saving: boolean;
  activeTab: SettingsTabId;
  /** Provider shown while `activeTab` is `providers`. */
  selectedProviderId: string | null;
  searchText: string;
  onSearchTextChange: (text: string) => void;
  onSelectPane: (tab: SettingsTabId) => void;
  onSelectProvider: (providerId: string) => void;
}

/**
 * The Settings sidebar (macOS `PreferencesSidebar`): search and the sort
 * toggle, the app panes with their colored chips, then the providers with an
 * "N on" count. Sorting only changes what the list shows; the saved provider
 * order changes through drag, Alt+Arrow or the row context menu.
 */
export function SettingsSidebar({
  settings,
  providers,
  snapshots,
  set,
  saving,
  activeTab,
  selectedProviderId,
  searchText,
  onSearchTextChange,
  onSelectPane,
  onSelectProvider,
}: SettingsSidebarProps) {
  const { t } = useLocale();
  // Local order so a reorder shows at once, before `reorder_providers` settles.
  const [ordered, setOrdered] = useState(providers);
  const [prevProviders, setPrevProviders] = useState(providers);
  if (providers !== prevProviders) {
    setPrevProviders(providers);
    setOrdered(providers);
  }

  const sorted = settings.providersSortedAlphabetically;
  const enabled = useMemo(() => new Set(settings.enabledProviders), [settings.enabledProviders]);
  const enabledCount = ordered.filter((p) => enabled.has(p.id)).length;

  const rows: ProviderSidebarRow[] = useMemo(() => {
    const snapshotMap = new Map(snapshots.map((s) => [s.providerId, s]));
    const shown = filterProvidersByQuery(
      sorted ? sortProvidersAlphabetically(ordered, enabled) : ordered,
      searchText,
    );
    return shown.map((p) => {
      const isOn = enabled.has(p.id);
      return {
        id: p.id,
        displayName: p.displayName,
        enabled: isOn,
        status: deriveProviderStatus(isOn, snapshotMap.get(p.id) ?? null),
      };
    });
  }, [enabled, ordered, searchText, snapshots, sorted]);

  const toggleEnabled = (id: string, on: boolean) => {
    set({ enabledProviders: nextEnabledProviders(ordered, enabled, id, on) });
  };

  const handleReorder = (ids: string[]) => {
    const byId = new Map(ordered.map((p) => [p.id, p]));
    setOrdered(ids.map((id) => byId.get(id)).filter((p): p is ProviderCatalogEntry => Boolean(p)));
    void reorderProviders(ids).catch(() => setOrdered(providers));
  };

  const sortHint = t(sorted ? "SettingsSidebarSortOnHint" : "SettingsSidebarSortOffHint");

  return (
    <aside className="settings-sidebar" aria-label={t("SettingsSidebarAriaLabel")}>
      <div className="settings-sidebar__toolbar">
        <div className="settings-sidebar__search">
          <svg
            className="settings-sidebar__search-icon"
            viewBox="0 0 16 16"
            width={12}
            height={12}
            aria-hidden
          >
            <circle cx="7" cy="7" r="4.5" />
            <path d="m10.5 10.5 3 3" />
          </svg>
          <input
            type="search"
            className="settings-sidebar__search-input"
            placeholder={t("ProviderSidebarSearch")}
            aria-label={t("ProviderSidebarSearch")}
            value={searchText}
            onChange={(e) => onSearchTextChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape" && searchText) {
                e.preventDefault();
                onSearchTextChange("");
              }
            }}
          />
          {searchText && (
            <button
              type="button"
              className="settings-sidebar__search-clear"
              aria-label={t("ProviderSidebarClearSearch")}
              title={t("ProviderSidebarClearSearch")}
              onClick={() => onSearchTextChange("")}
            >
              <svg viewBox="0 0 16 16" width={10} height={10} aria-hidden>
                <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" />
              </svg>
            </button>
          )}
        </div>
        <button
          type="button"
          className={`settings-sidebar__sort${sorted ? " settings-sidebar__sort--on" : ""}`}
          aria-pressed={sorted}
          aria-label={t("SettingsSidebarSortProviders")}
          title={sortHint}
          disabled={saving}
          onClick={() => set({ providersSortedAlphabetically: !sorted })}
        >
          <svg viewBox="0 0 16 16" width={14} height={14} aria-hidden>
            <path d="M5 3v10M2.5 10.5 5 13l2.5-2.5M11 13V3M8.5 5.5 11 3l2.5 2.5" />
          </svg>
        </button>
      </div>

      <div className="settings-sidebar__scroll">
        <ul className="settings-sidebar__panes">
          {SIDEBAR_PANES.map((pane) => {
            const active = activeTab === pane.id;
            return (
              <li key={pane.id}>
                <button
                  type="button"
                  className={`settings-sidebar__pane${active ? " settings-sidebar__pane--active" : ""}`}
                  aria-current={active ? "page" : undefined}
                  data-pane-id={pane.id}
                  onClick={() => onSelectPane(pane.id)}
                >
                  <PaneChip pane={pane} />
                  <span className="settings-sidebar__label">{t(pane.labelKey)}</span>
                </button>
              </li>
            );
          })}
        </ul>

        <div className="settings-sidebar__section-header">
          <span>{t("TabProviders")}</span>
          <span className="settings-sidebar__count">
            {t("SettingsSidebarProvidersOnCount").replace("{}", String(enabledCount))}
          </span>
        </div>
        <ProvidersSidebar
          providers={rows}
          selectedId={activeTab === "providers" ? selectedProviderId : null}
          canReorder={canReorderProviders(sorted, searchText)}
          onSelect={onSelectProvider}
          onReorder={handleReorder}
          onToggleEnabled={toggleEnabled}
          disabled={saving}
        />
      </div>
    </aside>
  );
}
