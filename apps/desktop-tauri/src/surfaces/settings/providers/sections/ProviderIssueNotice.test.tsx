import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ProviderDetail } from "../../../../types/bridge";
import { ProviderIssueNotice } from "./ProviderIssueNotice";

const detail = {
  id: "cursor",
  displayName: "Cursor",
  errorState: "needsAuthentication",
} as ProviderDetail;

const strings: Record<string, string> = {
  ProviderIssueAuthRequired: "Sign-in required",
  ProviderIssueUnknown: "Usage unavailable",
  ProviderIssuePrivacySafeDetail: "Details are hidden here to protect account data.",
  ProviderLastFetchFailedTitle: "Last {} fetch failed:",
  ProviderShowDetails: "Show details",
  ProviderHideDetails: "Hide details",
  ActionCopyError: "Copy error",
};
const t = vi.fn((key: string) => strings[key] ?? key);

describe("ProviderIssueNotice", () => {
  it("shows the Mac title and the privacy-safe preview, hiding the diagnostic until asked", () => {
    const raw = "authentication required for me@example.com at https://private.example.test";
    render(<ProviderIssueNotice detail={{ ...detail, lastError: raw }} t={t} />);

    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("Last Cursor fetch failed:");
    expect(status).toHaveTextContent("Sign-in required");
    expect(status).toHaveTextContent("Details are hidden here to protect account data.");
    expect(status).not.toHaveTextContent(/private\.example\.test/);
    expect(screen.getByRole("button", { name: "Copy error" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Show details" }));
    expect(status).toHaveTextContent(raw);
    expect(screen.getByRole("button", { name: "Hide details" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );

    fireEvent.click(screen.getByRole("button", { name: "Hide details" }));
    expect(status).not.toHaveTextContent(/private\.example\.test/);
  });

  it("masks emails in the expanded details under Hide Personal Info", () => {
    render(
      <ProviderIssueNotice
        detail={{ ...detail, lastError: "token expired for person@example.com" }}
        t={t}
        hidePersonalInfo
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Show details" }));
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("token expired for");
    expect(status).not.toHaveTextContent("person@example.com");
    expect(status.textContent).not.toContain("@");
    expect(status.textContent).not.toContain("example.com");
  });

  it("falls back to the unknown state and offers no details without a diagnostic", () => {
    render(<ProviderIssueNotice detail={{ ...detail, errorState: null, lastError: null }} t={t} />);

    expect(screen.getByRole("status")).toHaveTextContent("Usage unavailable");
    expect(screen.queryByRole("button", { name: "Show details" })).toBeNull();
  });
});
