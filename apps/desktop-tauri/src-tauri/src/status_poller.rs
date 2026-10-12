//! Desktop provider-status poller. Upstream polls status pages inside every
//! provider refresh while "Check provider status" is on
//! (`UsageStore.swift:842`, `UsageStore+ProviderStatus.swift:5-38`); the
//! shell does the same by starting a status poll from each full provider
//! refresh (unforced ones at most every `UNFORCED_POLL_SPACING`) and when
//! the switch turns on, but not from the Windows-only auto-resume probe.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use codexbar::core::ProviderId;
use codexbar::locale::{LocaleKey, get_text};
use codexbar::settings::Language;
use codexbar::settings::Settings;
use codexbar::status::{ProviderStatus, StatusIndicator, StatusSource, status_source};
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::{ProviderServiceStatus, ProviderStatusEntry};
use crate::events;
use crate::state::AppState;

/// Outer cap per source. `fetch_status` already times out at 10s like
/// upstream; this also bounds connect stalls the client timeout misses.
const POLL_TIMEOUT: Duration = Duration::from_secs(15);

/// Unforced refreshes run far more often on Windows than on the Mac: every
/// surface mount and the startup timer call the stale check, and any enabled
/// provider without a fresh snapshot makes each of those a full refresh. This
/// floor keeps those from re-polling a page; it stays below the 60-second
/// shortest refresh cadence minus the 15-second scheduler tick, so timed
/// refreshes still poll every round. Forced refreshes (manual, menu-open,
/// settings changes) poll every time, as upstream does.
const UNFORCED_POLL_SPACING: Duration = Duration::from_secs(30);

type FetchResult = Result<ProviderStatus, String>;

/// Last known status per provider. Every poll takes a ticket; a result only
/// lands when its ticket is newer than what the provider already has and was
/// issued after the last `clear`, so a poll still in flight when the switch
/// turns off can never publish.
#[derive(Debug, Default)]
pub struct ProviderStatusCache {
    statuses: HashMap<ProviderId, ProviderStatus>,
    issued: u64,
    applied: HashMap<ProviderId, u64>,
    floor: u64,
    started: HashMap<ProviderId, Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusTicket {
    provider: ProviderId,
    seq: u64,
}

impl ProviderStatusCache {
    fn issue(&mut self, provider: ProviderId) -> StatusTicket {
        self.issued += 1;
        StatusTicket {
            provider,
            seq: self.issued,
        }
    }

    /// A failed fetch keeps the last good status and, as upstream does, does
    /// not retire older polls still in flight: only a success is evidence
    /// (`UsageStore+ProviderStatus.swift:28-36`). Returns whether the
    /// published status changed.
    fn apply(&mut self, ticket: StatusTicket, result: FetchResult) -> bool {
        let stale = ticket.seq <= self.floor
            || self
                .applied
                .get(&ticket.provider)
                .is_some_and(|seq| *seq >= ticket.seq);
        let Ok(status) = result else {
            return false;
        };
        if stale {
            return false;
        }
        self.applied.insert(ticket.provider, ticket.seq);
        self.statuses.insert(ticket.provider, status.clone()) != Some(status)
    }

    fn clear(&mut self) {
        self.floor = self.issued;
        self.statuses.clear();
        self.applied.clear();
        self.started.clear();
    }

    fn retain(&mut self, providers: &[ProviderId]) {
        self.statuses.retain(|id, _| providers.contains(id));
    }

    pub fn get(&self, provider: ProviderId) -> Option<&ProviderStatus> {
        self.statuses.get(&provider)
    }
}

/// The providers to poll: the requested ones that are enabled and have a
/// status source, and none while the switch is off.
fn targets(settings: &Settings, providers: &[ProviderId]) -> Vec<(ProviderId, StatusSource)> {
    if !settings.status_checks_enabled {
        return Vec::new();
    }
    let enabled = settings.get_enabled_provider_ids();
    providers
        .iter()
        .filter(|id| enabled.contains(id))
        .filter_map(|id| status_source(*id).map(|source| (*id, source)))
        .collect()
}

/// Bridge entries for every enabled provider with a status source, in
/// display order. Empty while the switch is off, so the frontend falls back
/// to the fetch-status dot.
pub fn entries(cache: &ProviderStatusCache, settings: &Settings) -> Vec<ProviderStatusEntry> {
    targets(settings, &settings.get_enabled_provider_ids())
        .into_iter()
        .map(|(id, _)| ProviderStatusEntry {
            provider_id: id.cli_name().to_string(),
            status: cache.get(id).map(ProviderServiceStatus::from),
        })
        .collect()
}

/// Issue tickets for the targets to poll now. An unforced poll skips
/// providers whose last poll started within `UNFORCED_POLL_SPACING`.
fn begin(
    cache: &mut ProviderStatusCache,
    targets: Vec<(ProviderId, StatusSource)>,
    now: Instant,
    force: bool,
) -> Vec<(StatusTicket, StatusSource)> {
    let mut tickets = Vec::with_capacity(targets.len());
    for (id, source) in targets {
        let recent = cache
            .started
            .get(&id)
            .is_some_and(|at| now.saturating_duration_since(*at) < UNFORCED_POLL_SPACING);
        if recent && !force {
            continue;
        }
        cache.started.insert(id, now);
        tickets.push((cache.issue(id), source));
    }
    tickets
}

/// Fetch each distinct source once (Codex and OpenAI API share a page,
/// Gemini and Antigravity share the Workspace feed), concurrently.
async fn poll<F, Fut>(
    tickets: Vec<(StatusTicket, StatusSource)>,
    fetch: F,
    limit: Duration,
) -> Vec<(StatusTicket, FetchResult)>
where
    F: Fn(StatusSource) -> Fut,
    Fut: Future<Output = FetchResult> + Send + 'static,
{
    let mut sources: Vec<StatusSource> = Vec::new();
    for (_, source) in &tickets {
        if !sources.contains(source) {
            sources.push(*source);
        }
    }
    let mut tasks = tokio::task::JoinSet::new();
    for source in sources {
        let request = fetch(source);
        tasks.spawn(async move {
            let result = tokio::time::timeout(limit, request)
                .await
                .unwrap_or_else(|_| Err("timeout".to_string()));
            (source, result)
        });
    }
    let mut fetched: HashMap<StatusSource, FetchResult> = HashMap::new();
    while let Some(joined) = tasks.join_next().await {
        if let Ok((source, result)) = joined {
            fetched.insert(source, result);
        }
    }
    tickets
        .into_iter()
        .map(|(ticket, source)| {
            let result = fetched
                .get(&source)
                .cloned()
                .unwrap_or_else(|| Err("fetch task failed".to_string()));
            (ticket, result)
        })
        .collect()
}

/// Apply results that are still wanted under the current settings. Returns
/// whether any published status changed.
fn finish(
    cache: &mut ProviderStatusCache,
    settings: &Settings,
    results: Vec<(StatusTicket, FetchResult)>,
) -> bool {
    let wanted: Vec<ProviderId> = targets(settings, &settings.get_enabled_provider_ids())
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let mut changed = false;
    for (ticket, result) in results {
        if !wanted.contains(&ticket.provider) {
            continue;
        }
        if let Err(error) = &result {
            tracing::debug!(provider = ticket.provider.cli_name(), %error, "provider status poll failed");
        }
        changed |= cache.apply(ticket, result);
    }
    changed
}

/// Start a detached status poll for `providers`. Never blocks the caller;
/// the task holds no lock across the network wait, so app exit is not held up.
pub fn spawn_poll(app: &AppHandle, settings: &Settings, providers: &[ProviderId], force: bool) {
    let targets = targets(settings, providers);
    if targets.is_empty() {
        return;
    }
    let Some(state) = app.try_state::<Mutex<AppState>>() else {
        return;
    };
    let tickets = match state.lock() {
        Ok(mut guard) => begin(&mut guard.provider_statuses, targets, Instant::now(), force),
        Err(_) => return,
    };
    if tickets.is_empty() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let results = poll(tickets, codexbar::status::fetch_status, POLL_TIMEOUT).await;
        let settings = Settings::load();
        let state = app.state::<Mutex<AppState>>();
        let changed = match state.lock() {
            Ok(mut guard) => finish(&mut guard.provider_statuses, &settings, results),
            Err(_) => false,
        };
        if changed {
            publish(&app, &settings);
        }
    });
}

/// React to a saved settings change that touched the switch or the enabled
/// providers. Off clears every status and invalidates polls in flight; on
/// drops disabled providers, and polls the rest when `poll_now` (the caller
/// is not about to start a provider refresh, which polls on its own).
pub fn settings_changed(app: &AppHandle, settings: &Settings, poll_now: bool) {
    if let Some(state) = app.try_state::<Mutex<AppState>>()
        && let Ok(mut guard) = state.lock()
    {
        if settings.status_checks_enabled {
            guard
                .provider_statuses
                .retain(&settings.get_enabled_provider_ids());
        } else {
            guard.provider_statuses.clear();
        }
    }
    publish(app, settings);
    if poll_now {
        spawn_poll(app, settings, &settings.get_enabled_provider_ids(), true);
    }
}

fn publish(app: &AppHandle, settings: &Settings) {
    let entries = app
        .try_state::<Mutex<AppState>>()
        .and_then(|state| {
            state
                .lock()
                .ok()
                .map(|guard| entries(&guard.provider_statuses, settings))
        })
        .unwrap_or_default();
    let _ = app.emit(events::PROVIDER_STATUSES_UPDATED, entries);
    crate::tray_bridge::refresh_tray_presentation(app);
}

/// Upstream `UsageStoreSupport.swift:4-15` labels.
fn indicator_label(indicator: StatusIndicator, lang: Language) -> String {
    let key = match indicator {
        StatusIndicator::Operational => LocaleKey::ServiceStatusOperational,
        StatusIndicator::Minor => LocaleKey::ServiceStatusPartialOutage,
        StatusIndicator::Major => LocaleKey::ServiceStatusMajorOutage,
        StatusIndicator::Critical => LocaleKey::ServiceStatusCriticalIssue,
        StatusIndicator::Maintenance => LocaleKey::ServiceStatusMaintenance,
        StatusIndicator::Unknown => LocaleKey::ServiceStatusUnknown,
    };
    get_text(lang, key)
}

/// Upstream overlays the menu-bar icon with a dot or an exclamation mark
/// while a provider has an incident (`IconRenderer.swift:1057-1100`). The
/// Windows tray icon is the usage bars, so each incident becomes a tooltip
/// line right under the title instead.
fn incident_lines(cache: &ProviderStatusCache, settings: &Settings) -> Vec<String> {
    targets(settings, &settings.get_enabled_provider_ids())
        .into_iter()
        .filter_map(|(id, _)| {
            let status = cache.get(id)?;
            status.indicator.has_issue().then(|| {
                format!(
                    "{}: {}",
                    id.display_name(),
                    indicator_label(status.indicator, settings.ui_language)
                )
            })
        })
        .collect()
}

pub fn with_incident_lines(app: &AppHandle, settings: &Settings, tooltip: String) -> String {
    let lines = app
        .try_state::<Mutex<AppState>>()
        .and_then(|state| {
            state
                .lock()
                .ok()
                .map(|guard| incident_lines(&guard.provider_statuses, settings))
        })
        .unwrap_or_default();
    insert_after_title(tooltip, &lines)
}

fn insert_after_title(tooltip: String, lines: &[String]) -> String {
    if lines.is_empty() {
        return tooltip;
    }
    let (title, rest) = match tooltip.split_once('\n') {
        Some((title, rest)) => (title, Some(rest)),
        None => (tooltip.as_str(), None),
    };
    let mut out = vec![title.to_string()];
    out.extend(lines.iter().cloned());
    out.extend(rest.map(str::to_string));
    out.join("\n")
}

#[tauri::command]
pub fn get_provider_statuses(app: AppHandle) -> Vec<ProviderStatusEntry> {
    let settings = Settings::load();
    let state = app.state::<Mutex<AppState>>();
    state
        .lock()
        .map(|guard| entries(&guard.provider_statuses, &settings))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn settings(on: bool, providers: &[ProviderId]) -> Settings {
        let mut settings = Settings {
            status_checks_enabled: on,
            ..Settings::default()
        };
        settings.enabled_providers = providers
            .iter()
            .map(|id| id.cli_name().to_string())
            .collect();
        settings
    }

    fn status(indicator: StatusIndicator, description: &str) -> ProviderStatus {
        ProviderStatus {
            indicator,
            description: Some(description.to_string()),
            updated_at: None,
        }
    }

    fn run<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn fake_fetch(
        calls: Arc<AtomicUsize>,
    ) -> impl Fn(StatusSource) -> std::pin::Pin<Box<dyn Future<Output = FetchResult> + Send>> {
        move |source| {
            calls.fetch_add(1, Ordering::SeqCst);
            let result = match source {
                StatusSource::Statuspage("https://status.claude.com") => {
                    Ok(status(StatusIndicator::Major, "Elevated errors on Opus"))
                }
                StatusSource::Statuspage("https://status.openai.com") => Ok(status(
                    StatusIndicator::Operational,
                    "All Systems Operational",
                )),
                _ => Err("request failed".to_string()),
            };
            Box::pin(async move { result })
        }
    }

    fn poll_once(cache: &mut ProviderStatusCache, settings: &Settings) -> (bool, usize) {
        let calls = Arc::new(AtomicUsize::new(0));
        let tickets = begin(
            cache,
            targets(settings, &settings.get_enabled_provider_ids()),
            Instant::now(),
            true,
        );
        let results = run(poll(tickets, fake_fetch(calls.clone()), POLL_TIMEOUT));
        (
            finish(cache, settings, results),
            calls.load(Ordering::SeqCst),
        )
    }

    #[test]
    fn switch_off_polls_nothing_and_publishes_no_entries() {
        let off = settings(false, &[ProviderId::Claude, ProviderId::Codex]);
        let mut cache = ProviderStatusCache::default();
        assert_eq!(poll_once(&mut cache, &off), (false, 0));
        assert!(entries(&cache, &off).is_empty());
    }

    #[test]
    fn switch_on_polls_each_source_once_and_publishes_entries() {
        let on = settings(
            true,
            &[
                ProviderId::Claude,
                ProviderId::Codex,
                ProviderId::OpenAIApi,
                ProviderId::Zai,
            ],
        );
        let mut cache = ProviderStatusCache::default();
        assert_eq!(poll_once(&mut cache, &on), (true, 2));

        let entries = entries(&cache, &on);
        let ids: Vec<&str> = entries.iter().map(|e| e.provider_id.as_str()).collect();
        assert_eq!(ids.len(), 3);
        assert!(!ids.contains(&"zai"));
        let claude = entries.iter().find(|e| e.provider_id == "claude").unwrap();
        assert_eq!(
            claude.status,
            Some(ProviderServiceStatus {
                indicator: "major",
                description: Some("Elevated errors on Opus".to_string()),
                updated_at: None,
            })
        );
        let codex = entries.iter().find(|e| e.provider_id == "codex").unwrap();
        assert_eq!(codex.status.as_ref().unwrap().indicator, "none");
    }

    #[test]
    fn failed_fetch_keeps_last_good_status_or_stays_unknown() {
        let on = settings(true, &[ProviderId::Claude, ProviderId::Copilot]);
        let mut cache = ProviderStatusCache::default();
        poll_once(&mut cache, &on);
        assert_eq!(cache.get(ProviderId::Copilot), None);

        let ticket = cache.issue(ProviderId::Claude);
        assert!(!cache.apply(ticket, Err("timeout".to_string())));
        assert_eq!(
            cache.get(ProviderId::Claude).map(|s| s.indicator),
            Some(StatusIndicator::Major)
        );
        let copilot = entries(&cache, &on)
            .into_iter()
            .find(|e| e.provider_id == "copilot")
            .unwrap();
        assert_eq!(copilot.status, None);
    }

    #[test]
    fn turning_off_mid_poll_drops_results_and_clears_statuses() {
        let on = settings(true, &[ProviderId::Claude]);
        let mut cache = ProviderStatusCache::default();
        poll_once(&mut cache, &on);

        let in_flight = begin(
            &mut cache,
            targets(&on, &[ProviderId::Claude]),
            Instant::now(),
            true,
        );
        cache.clear();
        let results = run(poll(in_flight, fake_fetch(Arc::default()), POLL_TIMEOUT));
        assert!(!finish(&mut cache, &on, results));
        assert_eq!(cache.get(ProviderId::Claude), None);
    }

    #[test]
    fn older_poll_cannot_overwrite_newer_result() {
        let mut cache = ProviderStatusCache::default();
        let older = cache.issue(ProviderId::Claude);
        let newer = cache.issue(ProviderId::Claude);
        assert!(cache.apply(newer, Ok(status(StatusIndicator::Critical, "Down"))));
        assert!(!cache.apply(older, Ok(status(StatusIndicator::Operational, "Up"))));
        assert_eq!(
            cache.get(ProviderId::Claude).map(|s| s.indicator),
            Some(StatusIndicator::Critical)
        );
    }

    #[test]
    fn unforced_polls_within_thirty_seconds_skip_the_page_and_forced_ones_do_not() {
        let on = settings(true, &[ProviderId::Claude, ProviderId::Codex]);
        let ids = on.get_enabled_provider_ids();
        let mut cache = ProviderStatusCache::default();
        let t0 = Instant::now();
        assert_eq!(begin(&mut cache, targets(&on, &ids), t0, false).len(), 2);

        let soon = t0 + Duration::from_secs(29);
        assert!(begin(&mut cache, targets(&on, &ids), soon, false).is_empty());
        assert_eq!(begin(&mut cache, targets(&on, &ids), soon, true).len(), 2);

        let later = soon + Duration::from_secs(30);
        assert_eq!(begin(&mut cache, targets(&on, &ids), later, false).len(), 2);

        cache.clear();
        assert_eq!(begin(&mut cache, targets(&on, &ids), later, false).len(), 2);
    }

    #[test]
    fn newer_failure_does_not_retire_an_older_success_still_in_flight() {
        let mut cache = ProviderStatusCache::default();
        let older = cache.issue(ProviderId::Claude);
        let newer = cache.issue(ProviderId::Claude);
        assert!(!cache.apply(newer, Err("timeout".to_string())));
        assert!(cache.apply(older, Ok(status(StatusIndicator::Major, "Down"))));
        assert_eq!(
            cache.get(ProviderId::Claude).map(|s| s.indicator),
            Some(StatusIndicator::Major)
        );
    }

    #[test]
    fn results_for_providers_disabled_mid_poll_are_dropped() {
        let both = settings(true, &[ProviderId::Claude, ProviderId::Codex]);
        let mut cache = ProviderStatusCache::default();
        let tickets = begin(
            &mut cache,
            targets(&both, &[ProviderId::Claude, ProviderId::Codex]),
            Instant::now(),
            true,
        );
        let results = run(poll(tickets, fake_fetch(Arc::default()), POLL_TIMEOUT));
        let claude_only = settings(true, &[ProviderId::Claude]);
        assert!(finish(&mut cache, &claude_only, results));
        assert!(cache.get(ProviderId::Claude).is_some());
        assert_eq!(cache.get(ProviderId::Codex), None);
    }

    #[test]
    fn incidents_become_tooltip_lines_under_the_title() {
        let mut on = settings(
            true,
            &[ProviderId::Claude, ProviderId::Codex, ProviderId::Copilot],
        );
        let mut cache = ProviderStatusCache::default();
        poll_once(&mut cache, &on);
        let ticket = cache.issue(ProviderId::Copilot);
        cache.apply(ticket, Ok(status(StatusIndicator::Maintenance, "Planned")));

        let lines = incident_lines(&cache, &on);
        assert_eq!(lines, ["Claude: Major outage", "Copilot: Maintenance"]);
        assert_eq!(
            insert_after_title("CodexBar\nClaude: 40% left".to_string(), &lines),
            "CodexBar\nClaude: Major outage\nCopilot: Maintenance\nClaude: 40% left"
        );

        on.ui_language = Language::Japanese;
        assert_eq!(incident_lines(&cache, &on)[0], "Claude: 重大な障害");
        on.status_checks_enabled = false;
        assert!(incident_lines(&cache, &on).is_empty());
    }

    #[test]
    fn hung_fetch_times_out_without_blocking_other_sources() {
        let on = settings(true, &[ProviderId::Claude, ProviderId::Codex]);
        let mut cache = ProviderStatusCache::default();
        let tickets = begin(
            &mut cache,
            targets(&on, &on.get_enabled_provider_ids()),
            Instant::now(),
            true,
        );
        let results = run(poll(
            tickets,
            |source| {
                Box::pin(async move {
                    if source == StatusSource::Statuspage("https://status.claude.com") {
                        std::future::pending::<()>().await;
                    }
                    Ok(status(StatusIndicator::Minor, "Degraded"))
                }) as std::pin::Pin<Box<dyn Future<Output = FetchResult> + Send>>
            },
            Duration::from_millis(50),
        ));
        let by_provider: HashMap<ProviderId, FetchResult> = results
            .into_iter()
            .map(|(ticket, result)| (ticket.provider, result))
            .collect();
        assert_eq!(by_provider[&ProviderId::Claude], Err("timeout".to_string()));
        assert_eq!(
            by_provider[&ProviderId::Codex]
                .as_ref()
                .map(|s| s.indicator),
            Ok(StatusIndicator::Minor)
        );
    }
}
