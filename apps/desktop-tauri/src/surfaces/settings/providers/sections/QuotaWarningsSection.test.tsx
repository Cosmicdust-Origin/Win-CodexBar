import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SettingsSnapshot } from "../../../../types/bridge";
import { QuotaWarningsSection } from "./QuotaWarningsSection";

const t = (key: string) => key;

type QuotaSettings = Pick<
  SettingsSnapshot,
  | "highUsageThreshold"
  | "criticalUsageThreshold"
  | "providerQuotaWarnings"
  | "showNotifications"
  | "quotaWarningMarkersVisible"
>;

function settings(extra: Partial<QuotaSettings> = {}): QuotaSettings {
  return {
    highUsageThreshold: 80,
    criticalUsageThreshold: 95,
    providerQuotaWarnings: {},
    showNotifications: true,
    quotaWarningMarkersVisible: true,
    ...extra,
  };
}

function renderSection(value: QuotaSettings, onChange = vi.fn()) {
  const view = render(
    <QuotaWarningsSection providerId="codex" settings={value} disabled={false} t={t} onChange={onChange} />,
  );
  return { ...view, onChange };
}

function row(window: "session" | "weekly") {
  return document.querySelector(`.quota-warning-row[data-window="${window}"]`) as HTMLElement;
}

describe("QuotaWarningsSection", () => {
  it("shows Global with the inherited thresholds by default", () => {
    renderSection(settings());
    const session = row("session");
    expect(within(session).getByRole("radio", { name: "QuotaWarningGlobal" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(session.textContent).toContain("QuotaWarningInherited");
    expect(screen.getByText("QuotaWarningProviderInherits")).toBeTruthy();
  });

  it("switches a window to Custom with the global values and keeps the other window", async () => {
    const { onChange } = renderSection(
      settings({ providerQuotaWarnings: { codex: { weekly: { enabled: false } } } }),
    );
    await act(async () =>
      fireEvent.click(within(row("session")).getByRole("radio", { name: "QuotaWarningCustom" })),
    );
    expect(onChange).toHaveBeenCalledWith({
      providerQuotaWarnings: {
        codex: { weekly: { enabled: false }, session: { enabled: true, high: 80, critical: 95 } },
      },
    });
  });

  it("edits Custom thresholds as used percentages clamped to 0-100", async () => {
    const { onChange } = renderSection(
      settings({
        providerQuotaWarnings: { codex: { session: { enabled: true, high: 70, critical: 90 } } },
      }),
    );
    const warning = screen.getByRole("spinbutton", { name: "ProviderSessionLabel QuotaWarningWarning" });
    expect(warning).toHaveValue(70);
    await act(async () => fireEvent.change(warning, { target: { value: "120" } }));
    expect(onChange).toHaveBeenLastCalledWith({
      providerQuotaWarnings: { codex: { session: { enabled: true, high: 100, critical: 90 } } },
    });
  });

  it("keeps custom values when turned Off and writes an empty window for Global", async () => {
    const { onChange } = renderSection(
      settings({
        providerQuotaWarnings: { codex: { session: { enabled: true, high: 70, critical: 90 } } },
      }),
    );
    await act(async () =>
      fireEvent.click(within(row("session")).getByRole("radio", { name: "QuotaWarningOff" })),
    );
    expect(onChange).toHaveBeenLastCalledWith({
      providerQuotaWarnings: { codex: { session: { enabled: false, high: 70, critical: 90 } } },
    });
    await act(async () =>
      fireEvent.click(within(row("session")).getByRole("radio", { name: "QuotaWarningGlobal" })),
    );
    expect(onChange).toHaveBeenLastCalledWith({
      providerQuotaWarnings: { codex: { session: {} } },
    });
  });

  it("dims and locks the group while notifications and markers are both off", () => {
    const { container } = renderSection(
      settings({ showNotifications: false, quotaWarningMarkersVisible: false }),
    );
    expect(container.querySelector(".provider-quota-warnings.is-dimmed")).toBeTruthy();
    expect(screen.getByText("QuotaWarningProviderDisabled")).toBeTruthy();
    for (const radio of screen.getAllByRole("radio")) expect(radio).toBeDisabled();
  });

  it("explains the markers-only state", () => {
    const { container } = renderSection(settings({ showNotifications: false }));
    expect(container.querySelector(".is-dimmed")).toBeNull();
    expect(screen.getByText("QuotaWarningProviderMarkersOnly")).toBeTruthy();
  });
});
