## Summary

Ports the Tavily provider from upstream CodexBar v0.74.1. Tavily is a search and AI API service. The provider fetches account plan, per-API-key, and pay-as-you-go credit usage from `GET https://api.tavily.com/usage` with bearer authentication.

This is the first provider in Lane A of the v0.74 port (stacked: Tavily → Exa → Linkup → TinyApi). Each PR targets main independently; PRs merge in lane order.

## Upstream Reference

- Upstream CodexBar v0.74.1 commit: `fca039015` (PR #4382 "Add search and utility providers")
- Upstream plugin: `Resources/Plugins/tavily.js`
- Upstream docs: `docs/tavily.md`
- Ported from: JavaScript plugin logic → native Rust provider module

## Implementation

- New provider module: `rust/src/providers/tavily/mod.rs` (310 lines)
- Tavily API: `GET https://api.tavily.com/usage` (bearer auth with `TAVILY_API_KEY` env/settings)
- Usage meters:
  - **Primary**: Account plan credits (used / limit, with remaining detail)
  - **Secondary**: API key specific credits (used / limit or unlimited)
  - **Tertiary** (when present): Pay-as-you-go credits (used / limit)
- Usage detail rows show plan name, limit, and remaining credits per scope
- Rate limit: 10 requests per 10 minutes (enforced by provider; no replay on 429)
- Identity: Plan name (not email or account ID)

## Wiring (Windows-Specific)

- Added `ProviderId::Tavily` enum variant (count: 96 → 97)
- Factory methods: cli_name, display_name, cookie_domain (None), from_cli_name, brand_color (#3B82F6)
- Provider factory: instantiate arm + pace_capability (UNSUPPORTED, no pace window)
- Token accounts: added to non-exhaustive match as API-key provider (returns None)
- Settings: API key catalog entry (not shown here; Settings/frontend changes separate)
- CLI: `codexbar usage --provider tavily --source oauth`

## Code Quality

- ✓ `cargo fmt --all`
- ✓ `cargo check --manifest-path rust/Cargo.toml` (52.24s)
- ⏳ `cargo test` (deferred to CI; timeout in local run)
- ⏳ `cargo clippy` (deferred to CI)
- ⏳ Tauri shell checks (deferred to CI)
- ⏳ Frontend checks: pnpm test, tsc, build (deferred to CI)

## Windows Differences from Mac

1. **Disabled by default**: Like all new providers, Tavily is disabled by default in Settings and must be explicitly enabled.
2. **Settings UI**: Windows adds Settings tab for manual API key entry (not shown in upstream plugin).
3. **Keyring/secure storage**: Windows stores API keys via DPAPI in `%config%/CodexBar/settings.json` (upstream uses macOS Keychain).
4. **Rate limit handling**: Windows honors the documented 10/10min limit; no automatic replay on 429.
5. **Error messages**: User-facing errors are friendly (no response bodies logged per AGENTS.md).

## Proof (Pending)

- Real-window validation: Tavily card in tray panel + Settings API key section (mock mode)
- Screenshots: Proof kit under W:/mac-parity/rig; proof mode `settings:menu` or `menu` (float bar)
- Mock server: No live API calls during proof
- CUA or manual capture: TBD after build

## Known Gaps / Open Questions

1. **Frontend integration**: Settings provider card and API key input not yet validated on Windows desktop
2. **Settings tab content**: API key section rendering and wording match expected layout
3. **Tray icon**: Tavily brand color rendering (blue #3B82F6) on light/dark bars
4. **Parse error handling**: Edge cases in malformed Tavily response (test coverage via CI)
5. **Locale strings**: Not yet added to i18n files (will add in follow-up if needed)

## Merge Notes

- Baseline: origin/main (a8fac16e6)
- Head: cf75e28ef ("Add Tavily provider")
- Next in lane: Exa (will branch from this PR's merge commit)
- CI: CircleCI on same-repo PRs; hosted Windows gate required
