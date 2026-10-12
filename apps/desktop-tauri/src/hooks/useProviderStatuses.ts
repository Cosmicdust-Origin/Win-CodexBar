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
    // A broadcast that lands before the initial read is newer than it.
    let heard = false;
    getProviderStatuses()
      .then((initial) => {
        if (active && !heard && Array.isArray(initial)) setEntries(initial);
      })
      .catch(() => {});
    Promise.resolve(
      listen<ProviderStatusEntry[]>(PROVIDER_STATUSES_UPDATED, (event) => {
        heard = true;
        setEntries(event.payload);
      }),
    )
      .then((fn) => {
        if (active) unlisten = fn;
        else fn?.();
      })
      .catch(() => {});
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  return entries;
}
