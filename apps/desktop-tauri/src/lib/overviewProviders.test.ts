import { describe, expect, it } from "vitest";
import {
  MERGED_OVERVIEW_PROVIDER_LIMIT,
  effectiveOverviewSelection,
  selectOverviewProviders,
  toggleOverviewProvider,
} from "./overviewProviders";

const ids = ["codex", "claude", "gemini", "cursor", "copilot", "zai", "kimi", "amp"];
const snaps = ids.map((providerId) => ({ providerId }));

describe("selectOverviewProviders", () => {
  it("shows the first six providers when nothing is selected", () => {
    expect(MERGED_OVERVIEW_PROVIDER_LIMIT).toBe(6);
    expect(selectOverviewProviders(snaps, []).map((p) => p.providerId)).toEqual(ids.slice(0, 6));
  });

  it("shows the selected providers in provider order", () => {
    expect(selectOverviewProviders(snaps, ["amp", "codex"]).map((p) => p.providerId)).toEqual(["codex", "amp"]);
  });

  it("falls back to the first six when no selected provider is present", () => {
    expect(selectOverviewProviders(snaps, ["gone"]).map((p) => p.providerId)).toEqual(ids.slice(0, 6));
  });

  it("never shows more than six even for a longer saved list", () => {
    expect(selectOverviewProviders(snaps, ids)).toHaveLength(6);
  });
});

describe("toggleOverviewProvider", () => {
  it("starts from the default six when unchecking", () => {
    expect(toggleOverviewProvider(ids, [], "claude", false)).toEqual(["codex", "gemini", "cursor", "copilot", "zai"]);
  });

  it("refuses a seventh provider", () => {
    expect(toggleOverviewProvider(ids, [], "kimi", true)).toEqual(ids.slice(0, 6));
  });

  it("adds below the limit and keeps provider order", () => {
    expect(toggleOverviewProvider(ids, ["amp"], "claude", true)).toEqual(["claude", "amp"]);
  });

  it("saves an empty list when the last provider is unchecked", () => {
    expect(toggleOverviewProvider(ids, ["amp"], "amp", false)).toEqual([]);
    expect(effectiveOverviewSelection(ids, [])).toEqual(ids.slice(0, 6));
  });
});
