import { useCallback, useEffect, useRef, useState } from "react";
import { Field, Select, SettingsGroup, Toggle } from "../../../components/FormControls";
import { useLocale } from "../../../hooks/useLocale";
import { getHooksConfig, saveHooksConfig } from "../../../lib/tauri";
import {
  HOOK_EVENT_TYPES,
  type HookRulePayload,
  type HooksConfigSnapshot,
  type ProviderCatalogEntry,
} from "../../../types/bridge";

// Settings > Hooks, after macOS `PreferencesHooksPane.swift`: one Enable
// switch with the trust warning, then the rules editor. Like the Mac, every
// edit saves at once; text fields save when they lose focus.

/** A new rule, as the Mac "Add rule" button makes it. */
export function newHookRule(): HookRulePayload {
  return {
    enabled: true,
    event: "quota_reached",
    events: [],
    provider: null,
    threshold: null,
    executable: "",
    arguments: [],
    timeoutSecs: 10,
  };
}

/** The event the picker shows; a hand-edited rule may only list `events`. */
export function ruleEvent(rule: HookRulePayload): string {
  return rule.event ?? rule.events[0] ?? "quota_reached";
}

/** A typed percentage as the stored fraction, clamped to 1..100 % (Mac `thresholdFraction`). */
export function thresholdFraction(percent: string): number | null {
  const trimmed = percent.trim();
  if (trimmed === "") return null;
  const value = Number(trimmed);
  if (!Number.isFinite(value)) return null;
  return Math.min(Math.max(value, 1), 100) / 100;
}

function thresholdPercent(threshold: number | null): string {
  return threshold === null ? "" : String(Math.round(threshold * 100_000) / 1_000);
}

interface DraftRule {
  /** React key only; never saved. */
  key: number;
  rule: HookRulePayload;
}

/** A text field that saves when it loses focus (or on Enter). */
function CommitInput({
  value,
  onCommit,
  placeholder,
  ariaLabel,
  disabled,
  className,
  inputMode,
}: {
  value: string;
  onCommit: (value: string) => void;
  placeholder?: string;
  ariaLabel: string;
  disabled?: boolean;
  className?: string;
  inputMode?: "decimal";
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <input
      type="text"
      className={className ? `text-input ${className}` : "text-input"}
      value={draft}
      placeholder={placeholder}
      aria-label={ariaLabel}
      disabled={disabled}
      inputMode={inputMode}
      spellCheck={false}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={() => {
        if (draft !== value) onCommit(draft);
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") event.currentTarget.blur();
      }}
    />
  );
}

function TrashGlyph() {
  return (
    <svg width={14} height={14} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M2.5 4h11M6 4V2.5h4V4M4 4l.7 9.5h6.6L12 4M6.8 6.5v5M9.2 6.5v5" />
    </svg>
  );
}

function MinusCircleGlyph() {
  return (
    <svg width={14} height={14} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" aria-hidden>
      <circle cx="8" cy="8" r="6" />
      <path d="M5 8h6" />
    </svg>
  );
}

function WarningGlyph() {
  return (
    <svg className="hooks-pane__warning-icon" width={13} height={13} viewBox="0 0 16 16" aria-hidden>
      <path d="M8 1.5 15 14H1Z" fill="currentColor" />
      <path d="M8 6v4" stroke="var(--surface-elevated, #fff)" strokeWidth={1.6} strokeLinecap="round" />
      <circle cx="8" cy="12" r="0.9" fill="var(--surface-elevated, #fff)" />
    </svg>
  );
}

function RuleRow({
  rule,
  providers,
  maxArguments,
  disabled,
  onChange,
  onDelete,
}: {
  rule: HookRulePayload;
  providers: ProviderCatalogEntry[];
  maxArguments: number;
  disabled: boolean;
  onChange: (rule: HookRulePayload) => void;
  onDelete: () => void;
}) {
  const { t } = useLocale();
  const event = ruleEvent(rule);
  const eventOptions = (HOOK_EVENT_TYPES as readonly string[]).includes(event)
    ? HOOK_EVENT_TYPES.map((name) => ({ value: name, label: name }))
    : [{ value: event, label: event }, ...HOOK_EVENT_TYPES.map((name) => ({ value: name, label: name }))];
  const providerOptions = [
    { value: "", label: t("HooksAnyProvider") },
    ...providers.map((provider) => ({ value: provider.id, label: provider.displayName })),
  ];
  if (rule.provider && !providers.some((provider) => provider.id === rule.provider)) {
    providerOptions.push({ value: rule.provider, label: rule.provider });
  }
  const setArguments = (args: string[]) => onChange({ ...rule, arguments: args });

  return (
    <div className="hooks-rule" data-testid="hook-rule">
      <div className="hooks-rule__header">
        <Toggle
          variant="switch"
          checked={rule.enabled}
          ariaLabel={t("HooksRuleEnabled")}
          disabled={disabled}
          onChange={(enabled) => onChange({ ...rule, enabled })}
        />
        <Select
          variant="menu"
          value={event}
          options={eventOptions}
          ariaLabel={t("HooksEvent")}
          disabled={disabled}
          onChange={(name) => onChange({ ...rule, event: name, events: [] })}
        />
        <Select
          variant="menu"
          value={rule.provider ?? ""}
          options={providerOptions}
          ariaLabel={t("HooksProvider")}
          disabled={disabled}
          onChange={(id) => onChange({ ...rule, provider: id === "" ? null : id })}
        />
        <span className="hooks-rule__spacer" />
        <button
          type="button"
          className="hooks-pane__icon-button hooks-pane__icon-button--destructive"
          aria-label={t("HooksDeleteRule")}
          title={t("HooksDeleteRule")}
          disabled={disabled}
          onClick={onDelete}
        >
          <TrashGlyph />
        </button>
      </div>

      {event === "quota_low" && (
        <div className="hooks-rule__threshold">
          <span>{t("HooksThreshold")}</span>
          <CommitInput
            className="hooks-rule__threshold-input"
            value={thresholdPercent(rule.threshold)}
            placeholder="90"
            ariaLabel={t("HooksThreshold")}
            inputMode="decimal"
            disabled={disabled}
            onCommit={(text) => onChange({ ...rule, threshold: thresholdFraction(text) })}
          />
          <span>%</span>
        </div>
      )}

      <CommitInput
        className="hooks-rule__mono"
        value={rule.executable}
        placeholder={t("HooksExecutablePlaceholder")}
        ariaLabel={t("HooksExecutable")}
        disabled={disabled}
        onCommit={(executable) => onChange({ ...rule, executable })}
      />

      <div className="hooks-rule__arguments">
        <div className="hooks-rule__arguments-header">
          <span>{t("HooksArguments")}</span>
          <button
            type="button"
            className="hooks-pane__link-button"
            disabled={disabled || rule.arguments.length >= maxArguments}
            onClick={() => setArguments([...rule.arguments, ""])}
          >
            + {t("HooksAddArgument")}
          </button>
        </div>
        {rule.arguments.map((argument, index) => (
          // Arguments have no identity of their own; position is the key.
          <div className="hooks-rule__argument" key={index}>
            <CommitInput
              className="hooks-rule__mono"
              value={argument}
              placeholder={t("HooksArgument")}
              ariaLabel={t("HooksArgument")}
              disabled={disabled}
              onCommit={(value) =>
                setArguments(rule.arguments.map((old, i) => (i === index ? value : old)))
              }
            />
            <button
              type="button"
              className="hooks-pane__icon-button"
              aria-label={t("HooksDeleteArgument")}
              title={t("HooksDeleteArgument")}
              disabled={disabled}
              onClick={() => setArguments(rule.arguments.filter((_, i) => i !== index))}
            >
              <MinusCircleGlyph />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

export default function HooksTab({ providers }: { providers: ProviderCatalogEntry[] }) {
  const { t } = useLocale();
  const [snapshot, setSnapshot] = useState<HooksConfigSnapshot | null>(null);
  const [rules, setRules] = useState<DraftRule[]>([]);
  const [saveError, setSaveError] = useState<string | null>(null);
  const nextKey = useRef(0);
  // Saves run one after another so a later edit never lands before an earlier one.
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  // Only the newest save's answer replaces the drafts.
  const latest = useRef(0);

  const keyed = useCallback(
    (list: HookRulePayload[], previous: DraftRule[] = []): DraftRule[] =>
      list.map((rule, index) => ({
        key:
          list.length === previous.length && previous[index]
            ? previous[index].key
            : nextKey.current++,
        rule,
      })),
    [],
  );

  const apply = useCallback(
    (next: HooksConfigSnapshot, previous: DraftRule[]) => {
      setSnapshot(next);
      setRules(keyed(next.rules, previous));
    },
    [keyed],
  );

  useEffect(() => {
    let alive = true;
    getHooksConfig()
      .then((loaded) => {
        if (alive) apply(loaded, []);
      })
      .catch((err) => {
        if (alive) setSaveError(String(err));
      });
    return () => {
      alive = false;
    };
  }, [apply]);

  const persist = useCallback(
    (next: DraftRule[], enabled?: boolean) => {
      setRules(next);
      setSaveError(null);
      const request = { enabled: enabled ?? null, rules: next.map((draft) => draft.rule) };
      const id = ++latest.current;
      queue.current = queue.current.then(() =>
        saveHooksConfig(request)
          .then((saved) => {
            if (id === latest.current) apply(saved, next);
          })
          .catch((err) => {
            setSaveError(String(err));
            return getHooksConfig()
              .then((reloaded) => {
                if (id === latest.current) apply(reloaded, []);
              })
              .catch(() => undefined);
          }),
      );
    },
    [apply],
  );

  const loadError = snapshot?.error ?? null;
  const locked = snapshot === null || loadError !== null;
  const maxRules = snapshot?.maxRules ?? 32;
  const maxArguments = snapshot?.maxArguments ?? 32;
  const partlyOn = snapshot !== null && snapshot.settingsEnabled !== snapshot.fileEnabled;

  return (
    <div className="hooks-pane">
      <SettingsGroup title={t("TabHooks")}>
        <Field label={t("HooksEnableLabel")} description={t("HooksEnableSubtitle")}>
          <Toggle
            variant="switch"
            checked={snapshot?.enabled ?? false}
            ariaLabel={t("HooksEnableLabel")}
            disabled={locked}
            onChange={(on) => persist(rules, on)}
          />
        </Field>
        <div className="settings-group__row hooks-pane__caption">
          <WarningGlyph />
          <span>{t("HooksTrustWarning")}</span>
        </div>
        {partlyOn && (
          <p className="settings-group__row hooks-pane__caption" role="note">
            {t("HooksPartlyOn")}
          </p>
        )}
      </SettingsGroup>

      <SettingsGroup
        title={t("HooksRulesHeader")}
        footer={snapshot ? <p>{t("HooksRulesFooter").replace("{}", snapshot.path)}</p> : undefined}
      >
        {loadError !== null && (
          <p className="settings-group__error" role="alert">
            {t("HooksLoadError").replace("{}", loadError)}
          </p>
        )}
        {rules.length === 0 && loadError === null && (
          <p className="settings-group__row hooks-pane__caption">{t("HooksEmpty")}</p>
        )}
        {rules.map((draft, index) => (
          <RuleRow
            key={draft.key}
            rule={draft.rule}
            providers={providers}
            maxArguments={maxArguments}
            disabled={locked}
            onChange={(rule) =>
              persist(rules.map((old, i) => (i === index ? { ...old, rule } : old)))
            }
            onDelete={() => persist(rules.filter((_, i) => i !== index))}
          />
        ))}
        <div className="settings-group__row">
          <button
            type="button"
            className="hooks-pane__link-button"
            disabled={locked || rules.length >= maxRules}
            onClick={() =>
              persist([...rules, { key: nextKey.current++, rule: newHookRule() }])
            }
          >
            + {t("HooksAddRule")}
          </button>
        </div>
        {saveError !== null && (
          <p className="settings-group__error" role="alert">
            {t("HooksSaveError").replace("{}", saveError)}
          </p>
        )}
      </SettingsGroup>
    </div>
  );
}
