import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ClaudeSwapPathField } from "./ClaudeSwapPathField";

const t = (key: string) => key;

describe("ClaudeSwapPathField", () => {
  it("saves a changed path on blur and on Enter, and skips unchanged values", async () => {
    const onSettingsChange = vi.fn();
    render(
      <ClaudeSwapPathField value="~/bin/cswap" disabled={false} t={t} onSettingsChange={onSettingsChange} />,
    );
    const input = screen.getByRole("textbox", { name: "ClaudeSwapExecutablePath" });
    expect((input as HTMLInputElement).value).toBe("~/bin/cswap");
    await act(async () => fireEvent.blur(input));
    expect(onSettingsChange).not.toHaveBeenCalled();

    await act(async () => {
      fireEvent.change(input, { target: { value: "  C:/tools/cswap.exe  " } });
      fireEvent.keyDown(input, { key: "Enter" });
    });
    expect(onSettingsChange).toHaveBeenLastCalledWith({
      claudeSwapExecutablePath: "C:/tools/cswap.exe",
    });
  });

  it("allows retrying the same path while the saved value has not changed", async () => {
    const onSettingsChange = vi.fn();
    render(<ClaudeSwapPathField value="" disabled={false} t={t} onSettingsChange={onSettingsChange} />);
    const input = screen.getByRole("textbox");
    await act(async () => {
      fireEvent.change(input, { target: { value: "C:/tools/cswap.exe" } });
      fireEvent.blur(input);
    });
    // A failed save leaves `value` unchanged, so the next blur saves again.
    await act(async () => fireEvent.blur(input));
    expect(onSettingsChange).toHaveBeenCalledTimes(2);
    expect(onSettingsChange).toHaveBeenLastCalledWith({
      claudeSwapExecutablePath: "C:/tools/cswap.exe",
    });
  });

  it("follows the saved value when it changes elsewhere", () => {
    const { rerender } = render(
      <ClaudeSwapPathField value="a" disabled={false} t={t} onSettingsChange={() => {}} />,
    );
    rerender(<ClaudeSwapPathField value="b" disabled t={t} onSettingsChange={() => {}} />);
    const input = screen.getByRole("textbox") as HTMLInputElement;
    expect(input.value).toBe("b");
    expect(input.disabled).toBe(true);
  });
});
