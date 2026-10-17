import { describe, expect, it } from "vitest";
import type { LocaleKey } from "../i18n/keys";
import type {
  DailyCostPoint,
  ProviderLocalUsageSummary,
  QuotaWindowHistoryPoint,
} from "../types/bridge";
import { inlineDashboardModel, shortModelName } from "./inlineDashboard";

const STRINGS: Partial<Record<LocaleKey, string>> = {
  PanelToday: "Today",
  PanelLatest: "Latest",
  PanelLatestTokens: "Latest tokens",
  PanelPeriodCost: "{} cost",
  PanelPeriodTokens: "{} tokens",
  CostPeriodShortDays: "{}d",
  CostPeriodShortMonthToDate: "MTD",
  CostPeriodShortAll: "All",
  PanelEstimatedValue: "Est. {}",
  PanelCurrentWindow: "Current window",
  PanelCurrentWindowTokens: "Current window tokens",
  PanelPreviousWindow: "Previous window",
  PanelWindowsAgo: "{} windows ago",
  PanelPartialEstimate: "Partial estimate",
  PanelTopModelPrefix: "Top model",
  PanelEstimatedFromLocalLogs: "Estimated from local logs",
  IncompleteRequestsLabel: "Incomplete",
  IncompleteRequestsDetail: "{} requests had no final usage",
  UsageSpendTokens: "tokens",
};
const t = (key: LocaleKey) => STRINGS[key] ?? key;

function summary(overrides: Partial<ProviderLocalUsageSummary> = {}): ProviderLocalUsageSummary {
  return {
    todayCost: 1.25,
    thirtyDayCost: 40,
    thirtyDayTokens: 12_400_000,
    periodCost: 40,
    periodTokens: 12_400_000,
    reportingPeriod: "rolling:30",
    latestTokens: 380_000,
    topModel: "gpt-5.1-codex",
    estimateNote: "Estimated from local logs",
    tokenCostUpdatedAtMs: 0,
    ...overrides,
  };
}

function week(overrides: Partial<QuotaWindowHistoryPoint>): QuotaWindowHistoryPoint {
  return {
    offset: 0,
    start: "2026-10-06T09:24:00Z",
    end: "2026-10-13T09:24:00Z",
    totalTokens: 2_100_000,
    totalCostUsd: 6.5,
    tokensAreComplete: true,
    costIsComplete: true,
    entryCount: 3,
    boundariesAreEstimated: false,
    ...overrides,
  };
}

const days: DailyCostPoint[] = [
  { date: "2026-10-09", value: 2 },
  { date: "2026-10-10", value: null },
  { date: "2026-10-11", value: 8 },
];

const base = {
  providerId: "codex",
  costHistory: days,
  tokensHistory: [{ date: "2026-10-11", tokens: 380_000 }],
  t,
  locale: "en-US",
  timeZone: "UTC",
};

describe("inlineDashboardModel", () => {
  it("returns nothing without a local usage summary", () => {
    expect(inlineDashboardModel({ ...base, summary: null, quotaHistory: null })).toBeNull();
  });

  it("orders the KPIs Today, period cost, latest tokens, period tokens without a current window", () => {
    const model = inlineDashboardModel({ ...base, summary: summary(), quotaHistory: null });
    expect(model?.kpis).toEqual([
      { title: "Today", value: "$1.25", emphasis: true },
      { title: "30d cost", value: "$40.00", emphasis: false },
      { title: "Latest tokens", value: "380K", emphasis: false },
      { title: "30d tokens", value: "12.4M", emphasis: false },
    ]);
    expect(model?.windows).toEqual([]);
  });

  it("inserts the current window KPIs when the history is longer than a week", () => {
    const model = inlineDashboardModel({
      ...base,
      summary: summary(),
      quotaHistory: [week({}), week({ offset: 1, totalCostUsd: 9, totalTokens: 3_000_000 })],
    });
    expect(model?.kpis.map((kpi) => [kpi.title, kpi.value, kpi.emphasis])).toEqual([
      ["Today", "$1.25", true],
      ["Current window", "$6.50", true],
      ["Latest tokens", "380K", false],
      ["Current window tokens", "2.1M", false],
      ["30d cost", "$40.00", false],
      ["30d tokens", "12.4M", false],
    ]);
  });

  it("relabels the history KPIs as the current window for a 7-day history", () => {
    const model = inlineDashboardModel({
      ...base,
      summary: summary({ reportingPeriod: "rolling:7" }),
      quotaHistory: [week({ costIsComplete: false })],
    });
    expect(model?.kpis.map((kpi) => [kpi.title, kpi.value, kpi.emphasis])).toEqual([
      ["Today", "$1.25", true],
      ["Current window", "≥ $6.50", true],
      ["Latest tokens", "380K", false],
      ["Current window tokens", "2.1M", false],
    ]);
  });

  it("prefixes estimated window boundaries with Est.", () => {
    const model = inlineDashboardModel({
      ...base,
      summary: summary(),
      quotaHistory: [week({ boundariesAreEstimated: true })],
    });
    expect(model?.kpis[1].title).toBe("Est. Current window");
    expect(model?.kpis[3].title).toBe("Est. Current window tokens");
    expect(model?.windows[0].range).toBe("Est. Oct 6, 9:24 AM – Oct 13, 9:24 AM");
  });

  it("builds up to four recent window rows with cost and tokens", () => {
    const model = inlineDashboardModel({
      ...base,
      summary: summary(),
      quotaHistory: [
        week({}),
        week({ offset: 1, tokensAreComplete: false }),
        week({ offset: 2, entryCount: 0 }),
        week({ offset: 3, totalTokens: null, totalCostUsd: 1 }),
        week({ offset: 4 }),
      ],
    });
    expect(model?.windows.map((row) => [row.title, row.value, row.note])).toEqual([
      ["Current window", "$6.50 · 2.1M", null],
      ["Previous window", "$6.50 · ≥ 2.1M", "Partial estimate"],
      ["3 windows ago", "$1.00", null],
    ]);
    expect(model?.windows[0].range).toBe("Oct 6, 9:24 AM – Oct 13, 9:24 AM");
  });

  it("keeps an empty current window but drops empty past windows", () => {
    const model = inlineDashboardModel({
      ...base,
      summary: summary(),
      quotaHistory: [
        week({ entryCount: 0, totalCostUsd: null, totalTokens: null }),
        week({ offset: 1, entryCount: 0 }),
      ],
    });
    expect(model?.windows.map((row) => [row.title, row.value])).toEqual([
      ["Current window", "—"],
    ]);
  });

  it("marks incomplete history totals and adds the incomplete note", () => {
    const model = inlineDashboardModel({
      ...base,
      summary: summary({ incompleteRequestCount: 2 }),
      quotaHistory: null,
    });
    expect(model?.kpis[1].value).toBe("$40.00 · Incomplete");
    expect(model?.kpis[3].value).toBe("12.4M · Incomplete");
    expect(model?.detailLines).toContain("Incomplete · 2 requests had no final usage");
  });

  it("lists the top model and the estimate note", () => {
    const model = inlineDashboardModel({
      ...base,
      summary: summary({ topModel: "claude-opus-4-5-20251101-extended" }),
      quotaHistory: null,
    });
    expect(model?.detailLines).toEqual([
      "Top model: claude-opus-4-5-20251101-…",
      "Estimated from local logs",
    ]);
  });

  it("builds chart points with a compact currency scale", () => {
    const model = inlineDashboardModel({ ...base, summary: summary(), quotaHistory: null });
    expect(model?.points.map((point) => [point.date, point.value])).toEqual([
      ["2026-10-09", 2],
      ["2026-10-10", null],
      ["2026-10-11", 8],
    ]);
    expect(model?.maxValue).toBe(8);
    expect(model?.scaleLabel).toBe("$8");
    expect(model?.points[2].title).toBe("2026-10-11: $8.00 · 380K tokens");
  });

  it("hides the chart when there is no cost history", () => {
    const model = inlineDashboardModel({
      ...base,
      costHistory: [],
      summary: summary(),
      quotaHistory: null,
    });
    expect(model?.points).toEqual([]);
    expect(model?.scaleLabel).toBeNull();
  });

  it("shows a tokens-only dashboard for Muse", () => {
    const model = inlineDashboardModel({
      ...base,
      providerId: "muse",
      summary: summary({ todayCost: null, periodCost: null }),
      quotaHistory: null,
    });
    expect(model?.kpis).toEqual([
      { title: "Latest", value: "380K tokens", emphasis: true },
      { title: "30d tokens", value: "12.4M", emphasis: false },
    ]);
  });

  it("uses dashes for missing values", () => {
    const model = inlineDashboardModel({
      ...base,
      summary: summary({ todayCost: null, periodCost: null, latestTokens: null, periodTokens: null }),
      quotaHistory: null,
    });
    expect(model?.kpis.map((kpi) => kpi.value)).toEqual(["—", "—", "—", "—"]);
  });
});

describe("shortModelName", () => {
  it("keeps names up to 26 characters", () => {
    expect(shortModelName("  gpt-5.1-codex  ")).toBe("gpt-5.1-codex");
    expect(shortModelName("a".repeat(26))).toBe("a".repeat(26));
  });

  it("truncates longer names to 25 characters and an ellipsis", () => {
    expect(shortModelName("b".repeat(27))).toBe(`${"b".repeat(25)}…`);
  });
});
