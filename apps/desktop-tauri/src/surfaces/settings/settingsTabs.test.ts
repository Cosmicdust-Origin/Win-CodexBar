import { describe, expect, it } from "vitest";

import type { SettingsTabId } from "../../types/bridge";

import {
  SIDEBAR_PANES,
  TAB_META,
  canReorderProviders,
  filterProvidersByQuery,
  isSettingsTab,
  parseSettingsTabRequest,
  providersTabRequest,
  sortProvidersAlphabetically,
  nextEnabledProviders,
} from "./settingsTabs";

describe("isSettingsTab", () => {
  it.each(["general", "providers", "notifications", "menuBar", "menu", "usageSpend", "advanced", "hooks", "about"])(
    "returns true for a known tab id (%s)",
    (id) => {
      expect(isSettingsTab(id)).toBe(true);
    },
  );

  it.each(["", "General", "usage", "providers ", "unknown", "dashboard"])(
    "returns false for an unknown value (%s)",
    (id) => {
      expect(isSettingsTab(id)).toBe(false);
    },
  );

  it("narrows to SettingsTabId on true", () => {
    const value: string = "general";
    if (isSettingsTab(value)) {
      // assigned to a SettingsTabId-typed const — would fail to compile if narrowing broke
      const _narrowed: SettingsTabId = value;
      expect(_narrowed).toBe("general");
    } else {
      expect.fail("expected narrowing to succeed");
    }
  });
});

describe("TAB_META", () => {
  it("has exactly one entry per SettingsTabId", () => {
    const declared = new Set(TAB_META.map((m) => m.id));
    // The union of all SettingsTabId literals must match the declared ids exactly.
    const expected: SettingsTabId[] = [
      "general",
      "providers",
      "notifications",
      "menuBar",
      "menu",
      "usageSpend",
      "advanced",
      "hooks",
      "about",
    ];
    expect(declared).toEqual(new Set(expected));
    expect(TAB_META).toHaveLength(expected.length);
  });

  it("has unique ids", () => {
    expect(new Set(TAB_META.map((m) => m.id)).size).toBe(TAB_META.length);
  });
});

describe("parseSettingsTabRequest", () => {
  it("reads a plain tab id with no provider", () => {
    expect(parseSettingsTabRequest("providers")).toEqual({ tab: "providers", providerId: null });
    expect(parseSettingsTabRequest("about")).toEqual({ tab: "about", providerId: null });
  });

  it("reads a provider deep link on the Providers tab", () => {
    expect(parseSettingsTabRequest("providers:claude")).toEqual({
      tab: "providers",
      providerId: "claude",
    });
  });

  it("ignores an empty provider id", () => {
    expect(parseSettingsTabRequest("providers:")).toEqual({ tab: "providers", providerId: null });
  });

  it("keeps an unknown provider id for the tab to resolve", () => {
    expect(parseSettingsTabRequest("providers:nope")).toEqual({
      tab: "providers",
      providerId: "nope",
    });
  });

  it("drops a provider id on any other tab", () => {
    expect(parseSettingsTabRequest("general:claude")).toEqual({ tab: "general", providerId: null });
  });

  it("returns null for an unknown tab", () => {
    expect(parseSettingsTabRequest("dashboard")).toBeNull();
    expect(parseSettingsTabRequest("dashboard:claude")).toBeNull();
    expect(parseSettingsTabRequest("")).toBeNull();
    expect(parseSettingsTabRequest(undefined)).toBeNull();
  });
});

describe("providersTabRequest", () => {
  it("builds the deep link that parseSettingsTabRequest reads", () => {
    expect(providersTabRequest("codex")).toBe("providers:codex");
    expect(parseSettingsTabRequest(providersTabRequest("grok"))).toEqual({
      tab: "providers",
      providerId: "grok",
    });
  });
});

describe("SIDEBAR_PANES", () => {
  it("lists the app panes in the macOS order with their chip colors", () => {
    expect(SIDEBAR_PANES.map((pane) => [pane.id, pane.chip])).toEqual([
      ["general", "gray"],
      ["usageSpend", "green"],
      ["notifications", "red"],
      ["menuBar", "blue"],
      ["menu", "teal"],
      ["advanced", "purple"],
      ["hooks", "orange"],
      ["about", null],
    ]);
  });

  it("covers every settings tab except the provider panes", () => {
    const paneIds = new Set<string>(SIDEBAR_PANES.map((pane) => pane.id));
    for (const tab of TAB_META) {
      expect(paneIds.has(tab.id)).toBe(tab.id !== "providers");
    }
  });
});

const catalog = [
  { id: "codex", displayName: "Codex" },
  { id: "zai", displayName: "z.ai" },
  { id: "claude", displayName: "Claude" },
  { id: "amp", displayName: "Amp" },
  { id: "clawrouter", displayName: "ClawRouter" },
];

describe("filterProvidersByQuery", () => {
  it("matches name or id, ignoring case and surrounding spaces", () => {
    expect(filterProvidersByQuery(catalog, "  CL ").map((p) => p.id)).toEqual([
      "claude",
      "clawrouter",
    ]);
    expect(filterProvidersByQuery(catalog, "zai").map((p) => p.id)).toEqual(["zai"]);
    expect(filterProvidersByQuery(catalog, "Z.A").map((p) => p.id)).toEqual(["zai"]);
  });

  it("keeps every provider for a blank query", () => {
    expect(filterProvidersByQuery(catalog, "   ")).toBe(catalog);
  });
});

describe("sortProvidersAlphabetically", () => {
  it("puts enabled providers first, each group by name ignoring case", () => {
    const sorted = sortProvidersAlphabetically(catalog, new Set(["zai", "codex"]));
    expect(sorted.map((p) => p.id)).toEqual(["codex", "zai", "amp", "claude", "clawrouter"]);
  });

  it("does not reorder its input", () => {
    const input = [...catalog];
    sortProvidersAlphabetically(input, new Set());
    expect(input).toEqual(catalog);
  });

  it("breaks name ties by id", () => {
    const twins = [
      { id: "b", displayName: "Same" },
      { id: "a", displayName: "same" },
    ];
    expect(sortProvidersAlphabetically(twins, new Set()).map((p) => p.id)).toEqual(["a", "b"]);
  });
});

describe("canReorderProviders", () => {
  it("allows reordering only in the saved order with no search", () => {
    expect(canReorderProviders(false, "")).toBe(true);
    expect(canReorderProviders(false, "  ")).toBe(true);
    expect(canReorderProviders(true, "")).toBe(false);
    expect(canReorderProviders(false, "cl")).toBe(false);
  });
});

describe("nextEnabledProviders", () => {
  const catalog = [{ id: "codex" }, { id: "claude" }, { id: "cursor" }];

  it("adds a provider in catalog order", () => {
    expect(nextEnabledProviders(catalog, ["cursor", "codex"], "claude", true)).toEqual([
      "codex",
      "claude",
      "cursor",
    ]);
  });

  it("removes a provider and drops ids missing from the catalog", () => {
    expect(nextEnabledProviders(catalog, ["codex", "gone", "cursor"], "codex", false)).toEqual([
      "cursor",
    ]);
  });
});
