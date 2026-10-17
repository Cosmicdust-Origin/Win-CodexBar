import type { CSSProperties } from "react";
import { relativeLuminance } from "./color";

export function accentSelectionStyle(accent: string | null): CSSProperties {
  const luminance = accent ? relativeLuminance(accent) : null;
  if (accent === null || luminance === null) return {};
  const whiteContrast = 1.05 / (luminance + 0.05);
  return {
    "--mac-selection-bg": accent,
    "--mac-selection-text": whiteContrast >= 4.5 ? "#fff" : "#000",
  } as CSSProperties;
}
