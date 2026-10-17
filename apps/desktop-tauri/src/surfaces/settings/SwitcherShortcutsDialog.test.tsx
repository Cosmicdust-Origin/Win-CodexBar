import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => key, language: "english" }),
}));

import SwitcherShortcutsDialog from "./SwitcherShortcutsDialog";
import type { SettingsSnapshot } from "../../types/bridge";

function renderDialog(onClose = vi.fn(), set = vi.fn()) {
  const opener = document.createElement("button");
  document.body.appendChild(opener);
  opener.focus();
  const view = render(
    <SwitcherShortcutsDialog
      settings={{ switcherShortcuts: { next: "ctrl+alt+2" } } as unknown as SettingsSnapshot}
      set={set as never}
      saving={false}
      onClose={onClose}
    />,
  );
  return { ...view, opener, onClose, set };
}

describe("SwitcherShortcutsDialog", () => {
  it("is a labelled modal dialog that takes focus and returns it on close", () => {
    const { opener, unmount } = renderDialog();
    const dialog = screen.getByRole("dialog", { name: "SwitcherShortcutsTitle" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveFocus();

    unmount();
    expect(opener).toHaveFocus();
    opener.remove();
  });

  it("keeps Tab and Shift+Tab inside the sheet", () => {
    const { onClose, opener } = renderDialog();
    const dialog = screen.getByRole("dialog");
    const done = screen.getByRole("button", { name: "DialogDone" });
    const firstRecord = screen.getByRole("button", {
      name: "SwitcherShortcutPrevious: ShortcutRecordButton",
    });

    // Tab from the last control wraps to the first one.
    done.focus();
    fireEvent.keyDown(done, { key: "Tab" });
    expect(firstRecord).toHaveFocus();

    // Shift+Tab from the first control wraps to Done.
    fireEvent.keyDown(firstRecord, { key: "Tab", shiftKey: true });
    expect(done).toHaveFocus();

    // From the sheet itself (focus on open), Shift+Tab lands on Done and Tab on the first control.
    dialog.focus();
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(done).toHaveFocus();
    dialog.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(firstRecord).toHaveFocus();

    // A Tab between two inner controls is left to the browser.
    const event = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    firstRecord.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(onClose).not.toHaveBeenCalled();
    opener.remove();
  });

  it("closes on Escape, on Done and on a backdrop click", () => {
    const { onClose, container, opener } = renderDialog();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "DialogDone" }));
    fireEvent.mouseDown(container.querySelector(".settings-dialog-backdrop") as Element);
    expect(onClose).toHaveBeenCalledTimes(3);
    opener.remove();
  });

  it("does not close when the press starts inside the sheet", () => {
    const { onClose, opener } = renderDialog();
    fireEvent.mouseDown(screen.getByRole("dialog"));
    expect(onClose).not.toHaveBeenCalled();
    opener.remove();
  });

  it("restores defaults through the section's Restore Defaults button", () => {
    const { set, opener } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "SwitcherShortcutReset" }));
    expect(set).toHaveBeenCalledWith({ switcherShortcuts: {} });
    opener.remove();
  });
});
