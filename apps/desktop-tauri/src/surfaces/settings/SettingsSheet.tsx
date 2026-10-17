import type React from "react";
import { useEffect, useId, useRef } from "react";
import { useLocale } from "../../hooks/useLocale";

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Keeps Tab and Shift+Tab inside the modal sheet. */
function trapTab(dialog: HTMLElement, event: React.KeyboardEvent) {
  const focusable = Array.from(dialog.querySelectorAll<HTMLElement>(FOCUSABLE));
  if (focusable.length === 0) {
    event.preventDefault();
    return;
  }
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  const active = document.activeElement;
  const outside = !focusable.includes(active as HTMLElement);
  if (event.shiftKey && (active === first || outside)) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && (active === last || outside)) {
    event.preventDefault();
    first.focus();
  }
}

/**
 * In-window modal sheet for a Settings sub-page. Tab stays inside the sheet,
 * Escape, a backdrop click or Done closes it, and focus returns to the control
 * that opened it. Changes inside save at once, like the rest of Settings.
 */
export default function SettingsSheet({
  title,
  onClose,
  children,
}: {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
}) {
  const { t } = useLocale();
  const titleId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    dialogRef.current?.focus();
    return () => opener?.focus?.();
  }, []);

  return (
    <div
      className="settings-dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        className="settings-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.stopPropagation();
            onClose();
          } else if (event.key === "Tab" && dialogRef.current) {
            trapTab(dialogRef.current, event);
          }
        }}
      >
        <div className="settings-dialog__header">
          <h2 id={titleId} className="settings-dialog__title">
            {title}
          </h2>
        </div>
        <div className="settings-dialog__body">{children}</div>
        <div className="settings-dialog__actions">
          <button
            type="button"
            className="credential-btn credential-btn--primary"
            onClick={onClose}
          >
            {t("DialogDone")}
          </button>
        </div>
      </div>
    </div>
  );
}
