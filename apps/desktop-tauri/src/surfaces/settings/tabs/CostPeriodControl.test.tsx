import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Field } from "../../../components/FormControls";
import CostPeriodControl from "./CostPeriodControl";

const EN: Record<string, string> = {
  CostPeriodHistoryWindow: "History window",
  CostPeriodMonthToDate: "Month to date",
  CostPeriodAll: "All available history",
  CostPeriodToday: "Today",
  CostPeriodLastDays: "Last {} days",
  CostPeriodCustom: "Custom days",
  CostPeriodCustomDays: "Custom days count",
};
const t = (key: string) => EN[key] ?? key;

function renderControl(value: string, onChange = vi.fn()) {
  const view = render(
    <Field label={t("CostPeriodHistoryWindow")}>
      <CostPeriodControl value={value} onChange={onChange} t={t} />
    </Field>,
  );
  return { ...view, onChange };
}

const picker = () => screen.getByRole("combobox", { name: "History window" }) as HTMLSelectElement;

describe("CostPeriodControl", () => {
  it("is named by its Field and shows the saved preset", () => {
    renderControl("month-to-date");
    expect(picker().value).toBe("month-to-date");
    expect(picker().className).toBe("select select--menu");
  });

  it("saves a preset choice", () => {
    const { onChange } = renderControl("month-to-date");
    fireEvent.change(picker(), { target: { value: "rolling:90" } });
    expect(onChange).toHaveBeenCalledWith("rolling:90");
  });

  it("offers a custom day count and saves one valid value on blur", () => {
    const { onChange } = renderControl("month-to-date");
    fireEvent.change(picker(), { target: { value: "custom" } });
    expect(onChange).not.toHaveBeenCalled();
    const input = screen.getByLabelText("Custom days count") as HTMLInputElement;

    fireEvent.change(input, { target: { value: "400" } });
    expect(input.getAttribute("aria-invalid")).toBe("true");
    fireEvent.change(input, { target: { value: "0" } });
    // Typing "1" on the way to "14" saves nothing and keeps the input open.
    fireEvent.change(input, { target: { value: "1" } });
    expect(screen.getByLabelText("Custom days count")).toBe(input);
    fireEvent.change(input, { target: { value: "14" } });
    expect(onChange).not.toHaveBeenCalled();

    fireEvent.blur(input);
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith("rolling:14");
    expect(input.getAttribute("aria-invalid")).toBe("false");
  });

  it("commits a valid custom day count on Enter", () => {
    const { onChange } = renderControl("month-to-date");
    fireEvent.change(picker(), { target: { value: "custom" } });
    const input = screen.getByLabelText("Custom days count");
    fireEvent.change(input, { target: { value: "45" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith("rolling:45");
  });

  it("starts in custom mode for a saved count that is not a preset", () => {
    renderControl("rolling:14");
    expect((screen.getByLabelText("Custom days count") as HTMLInputElement).value).toBe("14");
    expect(picker().value).toBe("custom");
  });

  it("follows a value saved elsewhere", () => {
    const { rerender } = renderControl("rolling:14");
    rerender(
      <Field label={t("CostPeriodHistoryWindow")}>
        <CostPeriodControl value="all" onChange={vi.fn()} t={t} />
      </Field>,
    );
    expect(picker().value).toBe("all");
    expect(screen.queryByLabelText("Custom days count")).toBeNull();
  });
});
