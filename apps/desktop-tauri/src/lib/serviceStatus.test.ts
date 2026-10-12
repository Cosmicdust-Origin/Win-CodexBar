import { describe, expect, it } from "vitest";
import type { LocaleKey } from "../i18n/keys";
import type { ProviderStatusEntry } from "../types/bridge";
import { serviceStatusLine } from "./serviceStatus";

const STRINGS: Partial<Record<LocaleKey, string>> = {
  ServiceStatusMajorOutage: "Major outage",
  ServiceStatusMaintenance: "Maintenance",
  UpdatedJustNow: "Updated just now",
  UpdatedHoursAgo: "{} hours ago",
};
const t = (key: LocaleKey) => STRINGS[key] ?? key;
const NOW = Date.UTC(2026, 9, 12, 12, 0, 0);

function entry(
  providerId: string,
  indicator: "none" | "major" | "maintenance",
  description: string | null,
  updatedAt: number | null,
): ProviderStatusEntry {
  return { providerId, status: { indicator, description, updatedAt } };
}

describe("serviceStatusLine", () => {
  it("joins the description and the feed's update time", () => {
    const entries = [entry("claude", "major", "Elevated errors", NOW - 3 * 3_600_000)];
    expect(serviceStatusLine(entries, "claude", t, NOW)).toBe("Elevated errors — 3 hours ago");
  });

  it("falls back to the indicator label for a blank description", () => {
    const entries = [entry("copilot", "maintenance", "   ", NOW - 10_000)];
    expect(serviceStatusLine(entries, "copilot", t, NOW)).toBe("Maintenance — Updated just now");
    expect(serviceStatusLine([entry("copilot", "major", null, null)], "copilot", t, NOW)).toBe(
      "Major outage",
    );
  });

  it("has no line when operational, unpolled, or for another provider", () => {
    const entries = [
      entry("codex", "none", "All Systems Operational", NOW),
      { providerId: "gemini", status: null },
      entry("claude", "major", "Down", NOW),
    ];
    expect(serviceStatusLine(entries, "codex", t, NOW)).toBeNull();
    expect(serviceStatusLine(entries, "gemini", t, NOW)).toBeNull();
    expect(serviceStatusLine(entries, "cursor", t, NOW)).toBeNull();
    expect(serviceStatusLine(entries, null, t, NOW)).toBeNull();
  });
});
