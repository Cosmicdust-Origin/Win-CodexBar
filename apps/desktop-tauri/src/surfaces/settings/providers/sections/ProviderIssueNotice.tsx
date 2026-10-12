import { useCallback, useEffect, useRef, useState } from "react";
import type { ProviderDetail } from "../../../../types/bridge";
import type { LocaleKey } from "../../../../i18n/keys";
import { describeProviderState } from "../../../../lib/providerState";
import { hideEmailsInText } from "../../../../lib/personalInfo";

interface Props {
  detail: Pick<ProviderDetail, "displayName" | "errorState" | "lastError">;
  t: (key: LocaleKey) => string;
  hidePersonalInfo?: boolean;
}

/**
 * Error group of the provider pane (macOS `ProviderErrorView`): "Last X fetch
 * failed:" with a copy button, a short preview, and Show/Hide details for the
 * full text. The preview is the backend's privacy-safe state label; the full
 * text is the backend-redacted `lastError`, with emails masked under Hide
 * Personal Info, and only appears once the user asks for it.
 */
export function ProviderIssueNotice({ detail, t, hidePersonalInfo = false }: Props) {
  const [expanded, setExpanded] = useState(false);
  const state = describeProviderState(detail.errorState ?? "unknown");
  const preview = t(state.labelKey);
  const raw = detail.lastError?.trim() || null;
  const full = raw && hidePersonalInfo ? hideEmailsInText(raw) : raw;
  const title = t("ProviderLastFetchFailedTitle").replace("{}", detail.displayName);

  return (
    <section className="settings-group provider-detail-group provider-detail-error-group">
      <div className="settings-group__box">
        <div className="provider-detail-error" role="status">
          <div className="provider-detail-error__header">
            <strong>{title}</strong>
            <ErrorCopyButton text={full ?? `${detail.displayName}: ${preview}`} t={t} />
          </div>
          <p className="provider-detail-error__preview">{preview}</p>
          {!expanded && <p>{t("ProviderIssuePrivacySafeDetail")}</p>}
          {full && (
            <button
              type="button"
              className="provider-detail-link-btn"
              aria-expanded={expanded}
              onClick={() => setExpanded((v) => !v)}
            >
              {expanded ? t("ProviderHideDetails") : t("ProviderShowDetails")}
            </button>
          )}
          {expanded && full && <p className="provider-detail-error__full">{full}</p>}
        </div>
      </div>
    </section>
  );
}

function ErrorCopyButton({ text, t }: { text: string; t: (key: LocaleKey) => string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (timer.current != null) window.clearTimeout(timer.current);
    },
    [],
  );
  const handleCopy = useCallback(() => {
    navigator.clipboard?.writeText(text).catch(() => {});
    setCopied(true);
    if (timer.current != null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setCopied(false), 900);
  }, [text]);
  const label = copied ? t("PanelCopied") : t("ActionCopyError");
  return (
    <button
      type="button"
      className="provider-detail-icon-btn"
      onClick={handleCopy}
      aria-label={label}
      title={label}
    >
      <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">
        {copied ? (
          <path d="M2.5 8.5l3.5 3.5 7.5-8" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
        ) : (
          <>
            <rect x="5" y="5" width="9" height="9" rx="1.5" stroke="currentColor" strokeWidth="1.3" />
            <path d="M11 3V2.5A1.5 1.5 0 009.5 1H2.5A1.5 1.5 0 001 2.5v7A1.5 1.5 0 002.5 11H3" stroke="currentColor" strokeWidth="1.3" />
          </>
        )}
      </svg>
    </button>
  );
}
