import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { OptionsSection, type OptionSettings } from "./OptionsSection";

vi.mock("../../../../lib/tauri", () => ({
  setProviderAutoResumeAfterQuotaReset: vi.fn(),
  setProviderOptionalDetails: vi.fn(),
}));

const t = (key: string) => key;

const detail = (id: string) => ({
  id,
  autoResumeAfterQuotaReset: false,
  autoResumeSupported: false,
  optionalDetailsEnabled: false,
  optionalDetailsSupported: false,
});

function settings(extra: Partial<OptionSettings> = {}): OptionSettings {
  return {
    codexOpenaiWebExtras: undefined,
    codexHistoricalTracking: false,
    codexExternalOauthSourcesAllowed: false,
    codexLocalSessionCostLedgerEnabled: false,
    codexOpenaiWebBatterySaverEnabled: false,
    claudeAvoidKeychainPrompts: false,
    claudeAllowReadingClaudeCodeCredentials: false,
    claudeSwapEnabled: false,
    disableKeychainAccess: false,
    ...extra,
  } as OptionSettings;
}

function toggle(label: string) {
  return screen.getByRole("checkbox", { name: label });
}

describe("OptionsSection", () => {
  it("lists the Codex options with web extras on by default and marks unused ones", () => {
    render(
      <OptionsSection
        detail={detail("codex")}
        settings={settings()}
        disabled={false}
        t={t}
        onSettingsChange={vi.fn()}
        onReload={vi.fn()}
      />,
    );
    const labels = Array.from(document.querySelectorAll(".settings-field__label")).map(
      (label) => label.textContent,
    );
    expect(labels).toEqual([
      "ProviderCodexLocalCostEstimates",
      "ProviderCodexHistoricalTracking",
      "ProviderCodexOpenAiWebExtras",
      "ProviderCodexExternalOauth",
      "ProviderCodexBatterySaver",
    ]);
    expect(toggle("ProviderCodexOpenAiWebExtras")).toBeChecked();
    expect(screen.getByText(/ProviderCodexHistoryHelp ProviderOptionNotUsedOnWindows/)).toBeTruthy();
    expect(screen.queryByText(/ProviderCodexExternalOauthHelp ProviderOptionNotUsedOnWindows/)).toBeNull();
  });

  it("hides the battery saver while OpenAI web extras are off", () => {
    render(
      <OptionsSection
        detail={detail("codex")}
        settings={settings({ codexOpenaiWebExtras: false })}
        disabled={false}
        t={t}
        onSettingsChange={vi.fn()}
        onReload={vi.fn()}
      />,
    );
    expect(screen.queryByText("ProviderCodexBatterySaver")).toBeNull();
  });

  it("writes the Codex option that was switched", async () => {
    const onSettingsChange = vi.fn();
    render(
      <OptionsSection
        detail={detail("codex")}
        settings={settings()}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={vi.fn()}
      />,
    );
    await act(async () => fireEvent.click(toggle("ProviderCodexExternalOauth")));
    expect(onSettingsChange).toHaveBeenCalledWith({ codexExternalOauthSourcesAllowed: true });
    await act(async () => fireEvent.click(toggle("ProviderCodexOpenAiWebExtras")));
    expect(onSettingsChange).toHaveBeenLastCalledWith({ codexOpenaiWebExtras: false });
  });

  it("lists the Claude options and locks Avoid keychain prompts under the Advanced switch", async () => {
    const onSettingsChange = vi.fn();
    const { rerender } = render(
      <OptionsSection
        detail={detail("claude")}
        settings={settings()}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={vi.fn()}
      />,
    );
    expect(toggle("ProviderClaudeAvoidKeychainPrompts")).toBeEnabled();
    await act(async () => fireEvent.click(toggle("ClaudeSwapEnable")));
    expect(onSettingsChange).toHaveBeenCalledWith({ claudeSwapEnabled: true });

    rerender(
      <OptionsSection
        detail={detail("claude")}
        settings={settings({ disableKeychainAccess: true })}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={vi.fn()}
      />,
    );
    expect(toggle("ProviderClaudeAvoidKeychainPrompts")).toBeDisabled();
    expect(toggle("ProviderClaudeAllowReadingClaudeCodeCredentials")).toBeEnabled();
  });

  it("renders an empty group for a provider without options", () => {
    const { container } = render(
      <OptionsSection
        detail={detail("gemini")}
        settings={settings()}
        disabled={false}
        t={t}
        onSettingsChange={vi.fn()}
        onReload={vi.fn()}
      />,
    );
    expect(container.querySelector(".settings-group__box")?.childElementCount).toBe(0);
  });
});
