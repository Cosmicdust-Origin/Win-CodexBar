import type { ClaudeAccount } from "../types/bridge";

export interface PrivateClaudeAccountLabel {
  label: string;
  tooltip: string;
}

/**
 * Assign opaque labels from stable account ids rather than the current array
 * order. The account list can be reordered when the active login changes, but
 * the saved account id remains source-owned and stable. The tray numbers
 * Claude and Grok accounts by the same rule (`ordinals_by_sorted_id`).
 */
export function buildAccountOrdinals(
  accounts: readonly { id: string }[],
): Record<string, number> {
  const ids = [...new Set(accounts.map((account) => account.id))].sort((left, right) =>
    left < right ? -1 : left > right ? 1 : 0,
  );
  return Object.fromEntries(ids.map((id, index) => [id, index + 1]));
}

export function buildPrivateClaudeAccountLabel(
  account: ClaudeAccount,
  ordinal: number | undefined,
  hidePersonalInfo: boolean,
  accountWord: string,
): PrivateClaudeAccountLabel {
  if (hidePersonalInfo) {
    const label = ordinal === undefined ? "••••" : `${accountWord} ${ordinal}`;
    return { label, tooltip: label };
  }

  return { label: account.email, tooltip: account.email };
}
