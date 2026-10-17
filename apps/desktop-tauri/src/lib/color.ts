const HEX_COLOR = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i;

function srgbByteToLinear(byte: number): number {
  const c = byte / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

function parseHex(hex: string): [number, number, number] | null {
  const match = HEX_COLOR.exec(hex);
  if (!match) return null;
  const [r, g, b] = match.slice(1).map((part) => parseInt(part, 16));
  return [r, g, b];
}

/** WCAG relative luminance of a `#rrggbb` color, or null for anything else. */
export function relativeLuminance(hex: string): number | null {
  const rgb = parseHex(hex);
  if (!rgb) return null;
  const [r, g, b] = rgb.map(srgbByteToLinear);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio between two `#rrggbb` colors, or null if either is not hex. */
export function contrastRatio(a: string, b: string): number | null {
  const la = relativeLuminance(a);
  const lb = relativeLuminance(b);
  if (la === null || lb === null) return null;
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/** Mix a `#rrggbb` color toward white by `amount` (0..1). */
export function mixWithWhite(hex: string, amount: number): string {
  const rgb = parseHex(hex);
  if (!rgb) return hex;
  return `#${rgb
    .map((c) => Math.round(c + (255 - c) * amount).toString(16).padStart(2, "0"))
    .join("")}`;
}
