import { describe, expect, it } from "vitest";
import { containsEmail, hideEmailsInText } from "./personalInfo";

describe("hideEmailsInText", () => {
  it.each([
    ["proof@example.com", ""],
    ["Signed in as proof@example.com.", "Signed in as."],
    ["Contact me at user@example.com or admin@test.org for help", "Contact me at or for help"],
    ["Account: proof@example.com, Pro", "Account:, Pro"],
    ["No email here", "No email here"],
  ])("removes emails from %j", (input, expected) => {
    const hidden = hideEmailsInText(input);
    expect(hidden).toBe(expected);
    expect(hidden).not.toContain("@");
    expect(hidden).not.toContain("example.com");
  });

  it("passes empty values through", () => {
    expect(hideEmailsInText(null)).toBeNull();
    expect(hideEmailsInText(undefined)).toBeUndefined();
  });

  it("detects emails", () => {
    expect(containsEmail("Proof@Example.COM's Organization")).toBe(true);
    expect(containsEmail("parity-github")).toBe(false);
  });
});
