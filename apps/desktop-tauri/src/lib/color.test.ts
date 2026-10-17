import { describe, expect, it } from "vitest";
import { contrastRatio, mixWithWhite, relativeLuminance } from "./color";

describe("color helpers", () => {
  it("computes WCAG luminance and contrast for hex colors", () => {
    expect(relativeLuminance("#ffffff")).toBe(1);
    expect(relativeLuminance("#000000")).toBe(0);
    expect(contrastRatio("#000000", "#FFFFFF")).toBe(21);
    expect(contrastRatio("#111111", "#222222")).toBeCloseTo(1.19, 2);
  });

  it("rejects anything that is not #rrggbb", () => {
    expect(relativeLuminance("accent")).toBeNull();
    expect(relativeLuminance("#fff")).toBeNull();
    expect(contrastRatio("var(--x)", "#222222")).toBeNull();
  });

  it("mixes toward white", () => {
    expect(mixWithWhite("#111111", 0)).toBe("#111111");
    expect(mixWithWhite("#111111", 0.4)).toBe("#707070");
    expect(mixWithWhite("#000000", 1)).toBe("#ffffff");
    expect(mixWithWhite("nope", 0.5)).toBe("nope");
  });
});
