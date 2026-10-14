import type {
  BootstrapState,
  ProviderCatalogEntry,
  ProviderUsageSnapshot,
  SettingsUpdate,
} from "../../../types/bridge";
import { ProviderDetailPane } from "../providers/ProviderDetailPane";

interface ProvidersTabProps {
  settings: BootstrapState["settings"];
  providers: ProviderCatalogEntry[];
  set: (patch: SettingsUpdate) => void;
  saving: boolean;
  /** Provider selected in the Settings sidebar. */
  providerId: string | null;
  /** Live usage snapshots, shared with the sidebar's status dots. */
  snapshots: ProviderUsageSnapshot[];
}

/**
 * Detail pane of one provider. The provider list itself lives in the
 * Settings sidebar (`SettingsSidebar`), as on macOS.
 */
export default function ProvidersTab({
  settings,
  providers,
  set,
  saving,
  providerId,
  snapshots,
}: ProvidersTabProps) {
  const entry = providers.find((p) => p.id === providerId) ?? null;
  const liveSnapshot =
    snapshots.find((snapshot) => snapshot.providerId === providerId) ?? null;

  return (
    <ProviderDetailPane
      providerId={entry?.id ?? null}
      settings={settings}
      catalog={providers}
      liveSnapshot={liveSnapshot}
      cookieDomain={entry?.cookieDomain ?? null}
      resetTimeRelative={settings.resetTimeRelative}
      showAsUsed={settings.showAsUsed}
      providerMetrics={settings.providerMetrics}
      copilotSeatCreditEntitlement={settings.copilotSeatCreditEntitlement}
      providerAccentColors={settings.providerAccentColors}
      wayfinderGatewayUrl={settings.wayfinderGatewayUrl ?? "http://127.0.0.1:8088"}
      hidePersonalInfo={settings.hidePersonalInfo}
      settingsDisabled={saving}
      onSettingsChange={set}
    />
  );
}
