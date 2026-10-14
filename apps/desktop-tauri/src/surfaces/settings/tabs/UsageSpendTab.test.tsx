import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getUsageSpendSummary: vi.fn(),
  writeUsageSpendExport: vi.fn(),
}));

vi.mock("../../../lib/tauri", () => tauriMocks);
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

const EN: Record<string, string> = {
  CostPeriodMonthToDate: "Month to date",
  CostPeriodAll: "All available history",
  CostPeriodToday: "Today",
  CostPeriodLastDays: "Last {} days",
  CostPeriodShortAll: "All",
  CostPeriodShortDays: "{}d",
  UsageSpendModelsPeriodCaption: "Models for: {}",
  UsageSpendProjectsPeriodCaption: "Projects for: {}",
  UsageSpendEstimatedSpend: "Estimated spend",
  UsageSpendTrackedTokens: "Tracked tokens",
  UsageSpendSources: "Sources",
  UsageSpendTrackCosts: "Track costs",
  UsageSpendTrackingOffTitle: "Cost tracking is off",
  UsageSpendCopyJson: "Copy JSON",
  UsageSpendPeriodLabel: "Time range",
};
vi.mock("../../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => EN[key] ?? key }),
}));

import UsageSpendTab from "./UsageSpendTab";
import type { TabProps } from "../settingsTabs";
import type { SettingsSnapshot, SpendContract, UsageSpendRow, UsageSpendSummary } from "../../../types/bridge";

function contract(reportingPeriod: string): SpendContract {
  return {
    providerId: "codex",
    historyDays: 30,
    reportingPeriod,
    knownCostUsd: 12,
    knownZero: false,
    provenance: "listPriceEstimate",
    priceCoverage: { priced: 1, unpriced: 0, unmetered: 0, estimated: 0 },
    priceCoverageRatio: 1,
    historyCoverageEstablished: true,
    tokenMix: { inputTokens: 700, outputTokens: 200, cacheReadTokens: null, cacheCreationTokens: null, reasoningTokens: null },
    conversationCount: 1,
    models: [{ model: "gpt-5", costUsd: 9, inputTokens: 700, outputTokens: 200, cacheReadTokens: 0, totalTokens: 900, customPricing: false }],
    projects: [],
    conversations: [],
    daily: [],
    hourlyActivity: [],
    projectSourceStatus: null,
    imports: [],
    customPricingActive: false,
  } as SpendContract;
}

function row(providerId: string, displayName: string, periodCost: number | null, periodTokens: number | null, currency = "USD"): UsageSpendRow {
  return {
    providerId,
    displayName,
    sevenDay: 1,
    thirtyDay: 2,
    periodCost,
    periodTokens,
    currency,
    source: "local",
    includedInOverview: true,
  };
}

function summary(reportingPeriod: string, costTrackingEnabled = true): UsageSpendSummary {
  return {
    rows: costTrackingEnabled
      ? [row("codex", "Codex", 9, 900), row("claude", "Claude", 3.5, 2_100), row("zai", "z.ai", 4, 50, "CNY")]
      : [],
    contract: contract(reportingPeriod),
    reportingPeriod,
    reportingDay: "2026-10-11",
    dashboardTimezone: "UTC",
    costTrackingEnabled,
  };
}

function renderTab(settings: Partial<SettingsSnapshot> = {}) {
  const set = vi.fn();
  const props = {
    settings: { costReportingPeriod: "rolling:7", spendDashboardPeriod: "month-to-date", ...settings } as SettingsSnapshot,
    set,
    saving: false,
  } satisfies TabProps;
  render(<UsageSpendTab {...props} />);
  return set;
}

function lastScanPeriod(): string | undefined {
  const calls = tauriMocks.getUsageSpendSummary.mock.calls;
  return calls[calls.length - 1]?.[0]?.period;
}

function stat(label: string, scope: HTMLElement = document.body): string | null | undefined {
  const term = within(scope).getAllByText(label, { selector: "dt" })[0];
  return term.nextElementSibling?.textContent;
}

describe("UsageSpendTab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getUsageSpendSummary.mockImplementation(
      async (options?: { period?: string }) => summary(options?.period ?? "rolling:30"),
    );
    Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
  });

  it("scans the dashboard period, not the menu History window, and saves a new segment", async () => {
    const set = renderTab();

    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalledTimes(1));
    expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalledWith({ period: "month-to-date", forceRefresh: false });
    const range = screen.getByRole("radiogroup", { name: "Time range" });
    expect(within(range).getByRole("radio", { name: "Month to date" }).getAttribute("aria-checked")).toBe("true");

    fireEvent.click(within(range).getByRole("radio", { name: "90d" }));
    expect(set).toHaveBeenCalledWith({ spendDashboardPeriod: "rolling:90" });
  });

  it("copies the menu History window when no dashboard period is saved", async () => {
    renderTab({ spendDashboardPeriod: undefined, costReportingPeriod: "all" });
    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalled());
    expect(lastScanPeriod()).toBe("all");
  });

  it("keeps a legacy custom window as its own active segment", async () => {
    renderTab({ spendDashboardPeriod: "rolling:14" });
    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalled());
    expect(lastScanPeriod()).toBe("rolling:14");
    const range = screen.getByRole("radiogroup", { name: "Time range" });
    expect(within(range).getAllByRole("radio")).toHaveLength(6);
    expect(within(range).getByRole("radio", { name: "14d" }).getAttribute("aria-checked")).toBe("true");
  });

  it("sums the stat strip across providers per currency", async () => {
    renderTab();
    await screen.findByText("USD");
    const usd = screen.getByText("USD").closest(".usage-spend__currency") as HTMLElement;
    const cny = screen.getByText("CNY").closest(".usage-spend__currency") as HTMLElement;

    expect(stat("Estimated spend", usd)).toBe("$12.50");
    expect(stat("Tracked tokens", usd)).toBe("3K");
    expect(stat("Sources", usd)).toBe("2");
    expect(stat("Sources", cny)).toBe("1");
  });

  it("leaves hidden sources out of the strip and out of Copy JSON", async () => {
    const set = renderTab({ spendDashboardHiddenSourceIds: ["claude"] });
    await screen.findByText("USD");
    const usd = screen.getByText("USD").closest(".usage-spend__currency") as HTMLElement;
    expect(stat("Estimated spend", usd)).toBe("$9.00");
    expect(stat("Sources", usd)).toBe("1");

    // The hidden source stays in the filter, unchecked, so it can be shown again.
    const claude = screen.getByRole("checkbox", { name: "Claude" }) as HTMLInputElement;
    expect(claude.checked).toBe(false);
    fireEvent.click(claude);
    expect(set).toHaveBeenCalledWith({ spendDashboardHiddenSourceIds: [] });

    fireEvent.click(screen.getByRole("button", { name: "Copy JSON" }));
    await waitFor(() => expect(navigator.clipboard.writeText).toHaveBeenCalledTimes(1));
    const copied = JSON.parse(vi.mocked(navigator.clipboard.writeText).mock.calls[0][0]) as UsageSpendSummary;
    expect(copied.rows.map((entry) => entry.providerId)).toEqual(["codex", "zai"]);
  });

  it("blanks the Codex detail when Codex is hidden", async () => {
    renderTab({ spendDashboardHiddenSourceIds: ["codex"] });
    await screen.findByText("USD");
    expect(screen.queryByText("UsageSpendTokenInput")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Copy JSON" }));
    await waitFor(() => expect(navigator.clipboard.writeText).toHaveBeenCalled());
    const copied = JSON.parse(vi.mocked(navigator.clipboard.writeText).mock.calls[0][0]) as UsageSpendSummary;
    expect(copied.rows.map((entry) => entry.providerId)).toEqual(["claude", "zai"]);
    expect(copied.contract.models).toEqual([]);
    expect(copied.contract.knownCostUsd).toBeNull();
  });

  it("shows Cost tracking is off and keeps the switch to turn it back on", async () => {
    tauriMocks.getUsageSpendSummary.mockImplementation(async (options?: { period?: string }) =>
      summary(options?.period ?? "rolling:30", false),
    );
    const set = renderTab({ costUsageEnabled: false });

    expect(await screen.findByText("Cost tracking is off")).toBeTruthy();
    expect(screen.queryByText("Estimated spend")).toBeNull();
    expect(screen.queryByRole("radiogroup", { name: "Time range" })).toBeNull();

    const toggle = screen.getByRole("checkbox", { name: "Track costs" }) as HTMLInputElement;
    expect(toggle.checked).toBe(false);
    fireEvent.click(toggle);
    expect(set).toHaveBeenCalledWith({ costUsageEnabled: true });
  });

  it("hides the OpenCodex switches and the Sources filter while Track costs is off (#825)", async () => {
    tauriMocks.getUsageSpendSummary.mockImplementation(async (options?: { period?: string }) =>
      summary(options?.period ?? "rolling:30", false),
    );
    renderTab({ costUsageEnabled: false, openCodexUsageLogsEnabled: true });
    expect(await screen.findByText("Cost tracking is off")).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: "Track costs" })).not.toBeChecked();
    expect(screen.queryByText("UsageSpendOpenCodexImport")).toBeNull();
    expect(screen.queryByText("UsageSpendHideNativeCodex")).toBeNull();
    expect(screen.queryByRole("group", { name: "Sources" })).toBeNull();
    expect(screen.queryByRole("checkbox", { name: "Codex" })).toBeNull();
  });

  it("shows the OpenCodex switches and the Sources filter while Track costs is on", async () => {
    renderTab({ costUsageEnabled: true, openCodexUsageLogsEnabled: true });
    await screen.findByText("USD");
    expect(screen.getByText("UsageSpendOpenCodexImport")).toBeInTheDocument();
    expect(screen.getByText("UsageSpendHideNativeCodex")).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Sources" })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Codex" })).toBeChecked();
  });

  it("leaves the History window and Cost summary pickers to Settings > Menu", async () => {
    renderTab();
    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalledTimes(1));

    expect(screen.queryByLabelText("CostPeriodHistoryWindow")).toBeNull();
    expect(screen.queryByText("CostSummaryDisplayStyle")).toBeNull();
  });
});
