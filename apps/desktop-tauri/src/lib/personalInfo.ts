/**
 * Hide Personal Info removes emails from what the user sees, with no
 * placeholder (Mac `PersonalInfoRedactor.swift:18`, `emailPlaceholder = ""`).
 */
const EMAIL_PATTERN = /[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}/.source;

export function containsEmail(value: string | null | undefined): boolean {
  return !!value && new RegExp(EMAIL_PATTERN, "i").test(value);
}

/**
 * Mac `PersonalInfoRedactor.redactEmails(in:)` (`PersonalInfoRedactor.swift:41-55`):
 * remove each email, then close the gap it leaves before punctuation and
 * collapse repeated spaces.
 */
export function hideEmailsInText(value: string | null): string | null;
export function hideEmailsInText(value: string | null | undefined): string | null | undefined;
export function hideEmailsInText(value: string | null | undefined): string | null | undefined {
  if (!value || !containsEmail(value)) return value;
  return value
    .replace(new RegExp(EMAIL_PATTERN, "gi"), "")
    .replace(/\s+([:.,;])/g, "$1")
    .replace(/\s{2,}/g, " ")
    .trim();
}
