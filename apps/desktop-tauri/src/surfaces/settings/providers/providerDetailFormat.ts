import type { ProviderDetail } from "../../../types/bridge";
import type { LocaleKey } from "../../../i18n/keys";

export function buildSubtitle(
  detail: ProviderDetail,
  t: (k: LocaleKey) => string,
): string {
  const parts: string[] = [];
  if (detail.sourceLabel) parts.push(detail.sourceLabel);
  if (detail.lastUpdated) {
    const ago = relativeAgo(detail.lastUpdated);
    if (ago) parts.push(`${t("DetailUpdatedPrefix")} ${ago}`);
  } else if (!detail.hasSnapshot) {
    parts.push(t("ProviderUsageNotFetchedYet"));
  }
  return parts.join(" • ");
}

/** Source shown before a provider's first fetch (macOS `UsageStore.sourceLabel(for:)` fallback). */
export function providerSourceHintShort(
  providerId: string,
  t: (k: LocaleKey) => string,
): string {
  switch (providerId.toLowerCase()) {
    case "cursor":
    case "factory":
    case "droid":
    case "kimi":
    case "kimik2":
    case "augment":
    case "opencode":
    case "amp":
    case "ollama":
    case "alibaba":
    case "infini":
    case "manus":
    case "mimo":
    case "zoommate":
    case "notion":
    case "t3chat":
    case "commandcode":
    case "raycast":
    case "langdock":
    case "lithosai":
    case "museai":
    case "workbuddy":
      return t("ProviderSourceWebShort");
    case "gemini":
    case "antigravity":
    case "jetbrains":
      return t("ProviderSourceCliShort");
    case "copilot":
      return t("ProviderSourceOauthShort");
    case "zai":
    case "vertexai":
    case "openrouter":
    case "bedrock":
    case "nanogpt":
    case "warp":
    case "deepinfra":
    case "aiand":
    case "zenmux":
    case "clinepass":
    case "neuralwatt":
    case "devpass":
    case "xkiro":
    case "doubao":
    case "stepfun":
    case "venice":
    case "openaiapi":
    case "elevenlabs":
    case "deepgram":
    case "groq":
    case "llmproxy":
    case "xai":
    case "fireworks":
    case "meta":
    case "vercel":
    case "synthetic":
    case "clawrouter":
    case "ibmbob":
      return t("ProviderSourceApiShort");
    case "kiro":
      return t("ProviderSourceKiroEnvShort");
    default:
      return t("ProviderSourceAutoShort");
  }
}

export function relativeAgo(iso: string): string | null {
  const t = new Date(iso).getTime();
  if (!Number.isFinite(t)) return null;
  const diff = Date.now() - t;
  const secs = Math.round(Math.abs(diff) / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.round(mins / 60);
  if (hrs < 24) return `${hrs}h`;
  return `${Math.round(hrs / 24)}d`;
}
