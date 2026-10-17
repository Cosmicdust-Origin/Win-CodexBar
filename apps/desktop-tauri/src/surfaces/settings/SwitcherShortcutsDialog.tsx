import { useLocale } from "../../hooks/useLocale";
import SettingsSheet from "./SettingsSheet";
import SwitcherShortcutsSection from "./SwitcherShortcutsSection";
import type { TabProps } from "./settingsTabs";

/**
 * In-window sheet for the provider-switcher keys, opened from
 * Settings > General > Portable preferences. Every change saves at once,
 * like the rest of Settings.
 */
export default function SwitcherShortcutsDialog({
  settings,
  set,
  saving,
  onClose,
}: TabProps & { onClose: () => void }) {
  const { t } = useLocale();
  return (
    <SettingsSheet title={t("SwitcherShortcutsTitle")} onClose={onClose}>
      <SwitcherShortcutsSection settings={settings} set={set} saving={saving} />
    </SettingsSheet>
  );
}
