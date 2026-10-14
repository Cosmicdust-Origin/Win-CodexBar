import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AccentColorSection } from "./AccentColorSection";

const t = (key: string) => key;

describe("AccentColorSection", () => {
  it("uses the native color input as the only color preview", () => {
    const { container } = render(
      <AccentColorSection providerId="codex" accentColor="#123456" t={t} onChange={vi.fn()} />,
    );

    expect(container.querySelector('input[type="color"]')).toHaveValue("#123456");
    expect(container.querySelector(".accent-color-swatch-row")).toBeNull();
    expect(container.querySelector(".accent-color-swatch")).toBeNull();
  });

  it("renders a titled group with the hex field before the color well", () => {
    const { container } = render(
      <AccentColorSection providerId="codex" accentColor={null} t={t} onChange={vi.fn()} />,
    );
    expect(screen.getByRole("heading", { name: "ProviderAccentColor" })).toBeTruthy();
    expect(screen.getByText("ProviderAccentColorHelper")).toBeTruthy();
    const inputs = Array.from(container.querySelectorAll(".accent-color-row input"));
    expect(inputs.map((input) => input.getAttribute("type"))).toEqual(["text", "color"]);
  });

  it("shows Use default only while an override is saved and clears it", async () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <AccentColorSection providerId="codex" accentColor={null} t={t} onChange={onChange} />,
    );
    expect(screen.queryByRole("button", { name: "ProviderAccentColorReset" })).toBeNull();

    rerender(<AccentColorSection providerId="codex" accentColor="#ABCDEF" t={t} onChange={onChange} />);
    expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe("#ABCDEF");
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "ProviderAccentColorReset" })),
    );
    expect(onChange).toHaveBeenCalledWith({ providerAccentColors: { codex: null } });
  });

  it("normalizes a typed hex value and rejects an invalid one", async () => {
    const onChange = vi.fn();
    render(<AccentColorSection providerId="codex" accentColor={null} t={t} onChange={onChange} />);
    const input = screen.getByRole("textbox");
    await act(async () => {
      fireEvent.change(input, { target: { value: "12ab34" } });
      fireEvent.keyDown(input, { key: "Enter" });
    });
    expect(onChange).toHaveBeenCalledWith({ providerAccentColors: { codex: "#12AB34" } });

    onChange.mockClear();
    await act(async () => {
      fireEvent.change(input, { target: { value: "zz" } });
      fireEvent.blur(input);
    });
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole("alert").textContent).toBe("ProviderAccentColorInvalid");
  });
});
