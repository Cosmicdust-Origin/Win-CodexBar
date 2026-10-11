import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { LocaleKey } from "../../../../i18n/keys";
import { IdentitySection } from "./IdentitySection";

type IdentityDetails = Parameters<typeof IdentitySection>[0]["provider"];

function detail(overrides: Partial<IdentityDetails> = {}): IdentityDetails {
  return {
    id: "openaiapi",
    displayName: "OpenAI API",
    organization: "Project: proj-private",
    plan: "Admin API: proj-private",
    email: null,
    authType: null,
    sourceLabel: null,
    ...overrides,
  };
}

const t = (key: LocaleKey) => key;

describe("IdentitySection OpenAI project privacy", () => {
  it("drops the project account row and masks the plan's project id when privacy is enabled", () => {
    const { container } = render(
      <IdentitySection
        provider={detail()}
        subtitle=""
        t={t}
        hidePersonalInfo
      />,
    );

    expect(screen.queryByText("Account")).toBeNull();
    expect(screen.getByText("Admin API: ••••")).toBeInTheDocument();
    expect(container.innerHTML).not.toContain("proj-private");
  });

  it("keeps project ids visible when privacy is disabled", () => {
    render(<IdentitySection provider={detail()} subtitle="" t={t} />);

    expect(screen.getAllByText(/proj-private/)).toHaveLength(2);
  });
});

describe("IdentitySection account privacy for every provider", () => {
  it("drops the account email of non-OpenAI providers when privacy is enabled", () => {
    const { container } = render(
      <IdentitySection
        provider={detail({ id: "codex", displayName: "Codex", email: "person@example.com", organization: null, plan: "Pro" })}
        subtitle=""
        t={t}
        hidePersonalInfo
      />,
    );

    expect(container.innerHTML).not.toContain("@");
    expect(container.innerHTML).not.toContain("example.com");
    expect(screen.queryByText("Account")).toBeNull();
    expect(screen.getByText("Pro")).toBeInTheDocument();
  });

  it("drops an organization label that embeds the email", () => {
    const { container } = render(
      <IdentitySection
        provider={detail({ id: "claude", displayName: "Claude", email: null, organization: "person@example.com's Organization", plan: null })}
        subtitle=""
        t={t}
        hidePersonalInfo
      />,
    );

    expect(container.innerHTML).not.toContain("@");
    expect(container.innerHTML).not.toContain("example.com");
    expect(screen.queryByText("Account")).toBeNull();
  });

  it("drops an organization label without an email too", () => {
    render(
      <IdentitySection
        provider={detail({ id: "claude", displayName: "Claude", email: null, organization: "Acme Research", plan: "Max" })}
        subtitle=""
        t={t}
        hidePersonalInfo
      />,
    );

    expect(screen.queryByText("Account")).toBeNull();
    expect(screen.queryByText("Acme Research")).toBeNull();
    expect(screen.getByText("Max")).toBeInTheDocument();
  });

  it("shows the account email when privacy is disabled", () => {
    render(
      <IdentitySection
        provider={detail({ id: "codex", displayName: "Codex", email: "person@example.com", organization: null, plan: null })}
        subtitle=""
        t={t}
      />,
    );

    expect(screen.getByText("person@example.com")).toBeInTheDocument();
  });
});

describe("IdentitySection source row", () => {
  it("falls back to the provider's source hint before the first fetch", () => {
    render(
      <IdentitySection
        provider={detail({ id: "langdock", displayName: "Langdock", organization: null, plan: null })}
        subtitle=""
        t={t}
      />,
    );
    expect(screen.getByText("ProviderSourceWebShort")).toBeInTheDocument();
  });

  it("prefers the fetched source label", () => {
    render(
      <IdentitySection
        provider={detail({ id: "synthetic", displayName: "Synthetic", organization: null, plan: null, sourceLabel: "api" })}
        subtitle=""
        t={t}
      />,
    );
    expect(screen.getByText("api")).toBeInTheDocument();
    expect(screen.queryByText("ProviderSourceApiShort")).toBeNull();
  });
});
