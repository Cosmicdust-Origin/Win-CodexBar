import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type {
  ProviderCatalogEntry,
  ProviderUsageSnapshot,
  SettingsSnapshot,
} from "../../../types/bridge";

vi.mock("../providers/ProviderDetailPane", () => ({
  ProviderDetailPane: ({
    providerId,
    liveSnapshot,
    cookieDomain,
  }: {
    providerId: string | null;
    liveSnapshot?: { providerId: string } | null;
    cookieDomain?: string | null;
  }) => (
    <div
      data-testid="detail-pane"
      data-live={liveSnapshot?.providerId ?? ""}
      data-cookie={cookieDomain ?? ""}
    >
      {providerId}
    </div>
  ),
}));

import ProvidersTab from "./ProvidersTab";

const codex: ProviderCatalogEntry = { id: "codex", displayName: "Codex", cookieDomain: null };
const claude: ProviderCatalogEntry = {
  id: "claude",
  displayName: "Claude",
  cookieDomain: "claude.ai",
};
const settings = {
  enabledProviders: ["codex", "claude"],
  resetTimeRelative: true,
  providerMetrics: {},
} as unknown as SettingsSnapshot;

const live = (providerId: string) => ({ providerId }) as unknown as ProviderUsageSnapshot;

describe("ProvidersTab", () => {
  it("shows the sidebar's provider with its own snapshot and cookie domain only", () => {
    render(
      <ProvidersTab
        settings={settings}
        providers={[codex, claude]}
        set={vi.fn()}
        saving={false}
        providerId="claude"
        snapshots={[live("codex"), live("claude")]}
      />,
    );
    const pane = screen.getByTestId("detail-pane");
    expect(pane).toHaveTextContent("claude");
    expect(pane).toHaveAttribute("data-live", "claude");
    expect(pane).toHaveAttribute("data-cookie", "claude.ai");
  });

  it("shows no provider for an id outside the catalog", () => {
    render(
      <ProvidersTab
        settings={settings}
        providers={[codex]}
        set={vi.fn()}
        saving={false}
        providerId="nope"
        snapshots={[live("codex")]}
      />,
    );
    const pane = screen.getByTestId("detail-pane");
    expect(pane).toHaveTextContent("");
    expect(pane).toHaveAttribute("data-live", "");
  });
});
