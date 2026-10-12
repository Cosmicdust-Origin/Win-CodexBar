import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ProviderStatusEntry } from "../types/bridge";
import { getProviderStatuses } from "../lib/tauri";

export const PROVIDER_STATUSES_UPDATED = "provider-statuses-updated";

/**
 * Provider service statuses from the shell's status poller. Empty while
 * "Check provider status" is off; the poller broadcasts the full list on
 * every change, so the payload replaces the local copy.
 */
export function useProviderStatuses(): ProviderStatusEntry[] {
  const [entries, setEntries] = useState<ProviderStatusEntry[]>([]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    let heard = false;

    // Tauri does not replay events emitted before a listener is registered,
    // so the snapshot is read only after registration; a broadcast that
    // lands while the read is in flight is newer than the read.
    const initialize = async () => {
      try {
        unlisten = await listen<ProviderStatusEntry[]>(PROVIDER_STATUSES_UPDATED, (event) => {
          heard = true;
          if (active) setEntries(event.payload);
        });
      } catch {
        // The snapshot below still provides the current statuses.
      }
      if (!active) {
        unlisten?.();
        return;
      }
      try {
        const initial = await getProviderStatuses();
        if (active && !heard && Array.isArray(initial)) setEntries(initial);
      } catch {
        // Keep whatever a broadcast delivered.
      }
    };

    void initialize();
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  return entries;
}
