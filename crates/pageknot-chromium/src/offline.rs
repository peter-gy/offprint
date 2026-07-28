use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;
use std::time::Duration;

use pageknot_browser::OfflineBrowserObservation;
use pageknot_model::{ErrorStage, PageKnotError, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{Mutex, broadcast};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::targets::{FrameTargetManager, SessionRegistry, block_network};
use crate::{CdpClient, CdpEvent};

const STABILITY_QUIET: Duration = Duration::from_millis(300);
const STABILITY_POLL: Duration = Duration::from_millis(50);
const MAXIMUM_PAGE_ERRORS: usize = 100;
const MAXIMUM_PAGE_ERROR_BYTES: usize = 1024;
const MAXIMUM_ATTEMPTED_URLS: usize = 1024;
const MAXIMUM_ATTEMPTED_URL_BYTES: usize = 256 * 1024;

const STABILITY_PROBE: &str = r#"(() => {
    const state = globalThis.__pageknotOfflineStability ||= {records: []};
    const documents = [];
    const visit = (current) => {
        if (!current || documents.includes(current)) return;
        documents.push(current);
        for (const frame of current.querySelectorAll("iframe,frame")) {
            try {
                if (frame.contentDocument) visit(frame.contentDocument);
            } catch {
            }
        }
    };
    visit(document);
    state.records = state.records.filter((record) => documents.includes(record.document));
    for (const current of documents) {
        if (state.records.some((record) => record.document === current)) continue;
        const record = {document: current, mutations: 0};
        record.observer = new MutationObserver(() => { record.mutations += 1; });
        record.observer.observe(current, {
            attributes: true,
            childList: true,
            characterData: true,
            subtree: true
        });
        state.records.push(record);
    }
    let pendingImages = 0;
    let brokenImages = 0;
    let pendingFonts = 0;
    let failedFonts = 0;
    let ready = true;
    for (const current of documents) {
        ready &&= current.readyState === "complete";
        for (const image of current.images) {
            const source = image.currentSrc || image.getAttribute("src") || "";
            if (!source) continue;
            const bounds = image.getBoundingClientRect();
            const deferredLazyImage =
                image.loading === "lazy" &&
                !image.complete &&
                (bounds.bottom <= 0 ||
                    bounds.top >= (current.defaultView?.innerHeight || 0));
            if (deferredLazyImage) continue;
            if (!image.complete) pendingImages += 1;
            else if (image.naturalWidth === 0) brokenImages += 1;
        }
        if (current.fonts) {
            if (current.fonts.status !== "loaded") pendingFonts += 1;
            for (const face of current.fonts) {
                if (face.status === "error") failedFonts += 1;
            }
        }
    }
    return {
        ready,
        documents: documents.length,
        mutations: state.records.reduce((total, record) => total + record.mutations, 0),
        pendingImages,
        brokenImages,
        pendingFonts,
        failedFonts
    };
})()"#;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
struct StabilityProbe {
    ready: bool,
    documents: u64,
    mutations: u64,
    pending_images: u64,
    broken_images: u64,
    pending_fonts: u64,
    failed_fonts: u64,
}

impl StabilityProbe {
    fn settled(&self) -> bool {
        self.ready
            && self.pending_images == 0
            && self.broken_images == 0
            && self.pending_fonts == 0
            && self.failed_fonts == 0
    }

    fn failures(&self, target: &str) -> Vec<String> {
        let mut failures = Vec::new();
        if !self.ready {
            failures.push(format!(
                "{target}: document readiness did not reach complete"
            ));
        }
        if self.pending_images > 0 {
            failures.push(format!(
                "{target}: {} image loads remained pending",
                self.pending_images
            ));
        }
        if self.broken_images > 0 {
            failures.push(format!(
                "{target}: {} images failed to decode or load",
                self.broken_images
            ));
        }
        if self.pending_fonts > 0 {
            failures.push(format!(
                "{target}: {} document font sets remained pending",
                self.pending_fonts
            ));
        }
        if self.failed_fonts > 0 {
            failures.push(format!(
                "{target}: {} font faces failed to load",
                self.failed_fonts
            ));
        }
        failures
    }
}

#[derive(Debug)]
pub(crate) struct OfflineVerifier<'a> {
    client: &'a CdpClient,
    main_session_id: &'a str,
    sessions: &'a SessionRegistry,
    targets: &'a FrameTargetManager,
    seen_sessions: Arc<Mutex<HashSet<String>>>,
}

#[derive(Debug, Default)]
struct OfflineEventState {
    attempted_urls: BTreeMap<String, AttemptedUrlEvidence>,
    page_errors: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Default)]
struct AttemptedUrlEvidence {
    urls: BTreeSet<String>,
    bytes: usize,
    overflow: bool,
}

impl AttemptedUrlEvidence {
    fn record(&mut self, url: &str) {
        if self.urls.contains(url) || self.overflow {
            return;
        }
        let Some(bytes) = self.bytes.checked_add(url.len()) else {
            self.overflow = true;
            return;
        };
        if self.urls.len() >= MAXIMUM_ATTEMPTED_URLS || bytes > MAXIMUM_ATTEMPTED_URL_BYTES {
            self.overflow = true;
            return;
        }
        self.bytes = bytes;
        self.urls.insert(url.to_owned());
    }
}

#[derive(Debug)]
pub(crate) struct OfflineEventStream {
    cancellation: CancellationToken,
    task: Option<JoinHandle<Result<OfflineEventState>>>,
}

impl OfflineEventStream {
    async fn stop(mut self) -> Result<OfflineEventState> {
        self.cancellation.cancel();
        let Some(task) = self.task.take() else {
            return Err(PageKnotError::new(
                "pageknot.browser.cdp_event_lag",
                ErrorStage::Verification,
                "offline verifier event collector is unavailable",
            ));
        };
        task.await.map_err(|error| {
            PageKnotError::new(
                "pageknot.browser.cdp_event_lag",
                ErrorStage::Verification,
                format!("offline verifier event collector failed: {error}"),
            )
        })?
    }
}

impl Drop for OfflineEventStream {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl<'a> OfflineVerifier<'a> {
    pub(crate) fn new(
        client: &'a CdpClient,
        main_session_id: &'a str,
        sessions: &'a SessionRegistry,
        targets: &'a FrameTargetManager,
    ) -> Self {
        Self {
            client,
            main_session_id,
            sessions,
            targets,
            seen_sessions: Arc::new(Mutex::new(HashSet::from([main_session_id.to_owned()]))),
        }
    }

    pub(crate) fn subscribe(&self) -> OfflineEventStream {
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let events = self.client.subscribe_offline();
        let task =
            tokio::spawn(async move { collect_offline_events(events, task_cancellation).await });
        OfflineEventStream {
            cancellation,
            task: Some(task),
        }
    }

    pub(crate) async fn block_network(&self) -> Result<()> {
        self.targets.block_network().await?;
        block_network(self.client, self.main_session_id).await
    }

    pub(crate) async fn fence_events(&self, deadline: Instant) -> Result<()> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        self.client
            .command_with_timeout(
                "Runtime.evaluate",
                json!({
                    "expression": "void 0",
                    "returnByValue": false,
                    "awaitPromise": false,
                    "silent": true,
                }),
                Some(self.main_session_id),
                remaining,
            )
            .await
            .map(|_| ())
            .map_err(|error| error.with_detail("verificationStep", "event-fence"))
    }

    pub(crate) async fn wait_for_stability(
        &self,
        deadline: Instant,
    ) -> Result<(bool, Vec<String>)> {
        let mut previous = None;
        let mut unchanged_since = None;
        let mut latest_failures = Vec::new();
        loop {
            let now = Instant::now();
            if now >= deadline {
                latest_failures
                    .push("offline verification exceeded its shared stability deadline".to_owned());
                latest_failures.sort();
                latest_failures.dedup();
                return Ok((false, latest_failures));
            }
            let sample = match self.sample(deadline).await {
                Ok(sample) => sample,
                Err(error) if error.code.as_str() == "pageknot.verification.frame" => {
                    return Ok((false, vec![error.message]));
                }
                Err(error) => return Err(error),
            };
            latest_failures = sample
                .iter()
                .flat_map(|(target, probe)| probe.failures(target))
                .collect();
            let all_settled = sample.values().all(StabilityProbe::settled);
            if all_settled && previous.as_ref() == Some(&sample) {
                let quiet_start = unchanged_since.get_or_insert(now);
                if now.duration_since(*quiet_start) >= STABILITY_QUIET {
                    return Ok((true, Vec::new()));
                }
            } else {
                unchanged_since = None;
            }
            previous = Some(sample);
            tokio::time::sleep(STABILITY_POLL.min(deadline.saturating_duration_since(now))).await;
        }
    }

    async fn sample(&self, deadline: Instant) -> Result<BTreeMap<String, StabilityProbe>> {
        let frames = self.targets.frames().await.map_err(|error| {
            PageKnotError::new(
                "pageknot.verification.frame",
                ErrorStage::Verification,
                format!("offline frame set became unstable: {}", error.message),
            )
        })?;
        let mut targets = Vec::with_capacity(frames.len() + 1);
        targets.push(("main-frame".to_owned(), self.main_session_id.to_owned()));
        targets.extend(
            frames
                .into_iter()
                .map(|frame| (format!("frame:{}", frame.target_id), frame.session_id)),
        );
        {
            let mut seen = self.seen_sessions.lock().await;
            seen.extend(targets.iter().map(|(_, session)| session.clone()));
            seen.extend(self.sessions.read().await.iter().cloned());
        }
        let futures = targets.into_iter().map(|(label, session_id)| async move {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(PageKnotError::new(
                    "pageknot.verification.timeout",
                    ErrorStage::Verification,
                    "offline stability probe exceeded its deadline",
                ));
            }
            let response = self
                .client
                .command_with_timeout(
                    "Runtime.evaluate",
                    json!({
                        "expression": STABILITY_PROBE,
                        "returnByValue": true,
                        "awaitPromise": false,
                        "userGesture": false,
                    }),
                    Some(&session_id),
                    remaining,
                )
                .await
                .map_err(|error| {
                    error
                        .with_detail("verificationStep", "frame-stability")
                        .with_detail("target", label.clone())
                })?;
            if let Some(exception) = response.get("exceptionDetails") {
                let reason = exception
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("offline stability probe failed");
                return Err(PageKnotError::new(
                    "pageknot.verification.frame",
                    ErrorStage::Verification,
                    format!("{label}: {reason}"),
                ));
            }
            let value = response.pointer("/result/value").cloned().ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.verification.frame",
                    ErrorStage::Verification,
                    format!("{label}: stability probe returned no value"),
                )
            })?;
            let probe = serde_json::from_value(value).map_err(|error| {
                PageKnotError::new(
                    "pageknot.verification.frame",
                    ErrorStage::Verification,
                    format!("{label}: invalid stability probe result: {error}"),
                )
            })?;
            Ok::<_, PageKnotError>((label, probe))
        });
        futures_util::future::try_join_all(futures)
            .await
            .map(|samples| samples.into_iter().collect())
    }

    pub(crate) async fn finish(
        &self,
        events: OfflineEventStream,
        stable: bool,
        mut page_errors: Vec<String>,
    ) -> Result<OfflineBrowserObservation> {
        let events = events.stop().await?;
        let seen_sessions = self.seen_sessions.lock().await.clone();
        let mut attempted_urls = BTreeSet::new();
        let mut attempted_url_bytes = 0_usize;
        for (session, evidence) in events.attempted_urls {
            if seen_sessions.contains(&session) {
                if evidence.overflow {
                    return Err(attempted_url_limit_error());
                }
                for url in evidence.urls {
                    if attempted_urls.contains(&url) {
                        continue;
                    }
                    attempted_url_bytes = attempted_url_bytes
                        .checked_add(url.len())
                        .ok_or_else(attempted_url_limit_error)?;
                    if attempted_urls.len() >= MAXIMUM_ATTEMPTED_URLS
                        || attempted_url_bytes > MAXIMUM_ATTEMPTED_URL_BYTES
                    {
                        return Err(attempted_url_limit_error());
                    }
                    attempted_urls.insert(url);
                }
            }
        }
        for (session, errors) in events.page_errors {
            if seen_sessions.contains(&session) {
                for error in errors {
                    push_page_error(&mut page_errors, &error);
                }
            }
        }
        page_errors.sort();
        page_errors.dedup();
        Ok(OfflineBrowserObservation {
            attempted_urls: attempted_urls.into_iter().collect(),
            page_errors,
            stable,
        })
    }
}

async fn collect_offline_events(
    mut events: broadcast::Receiver<CdpEvent>,
    cancellation: CancellationToken,
) -> Result<OfflineEventState> {
    let mut state = OfflineEventState::default();
    loop {
        tokio::select! {
            () = cancellation.cancelled() => {
                let pending = events.len();
                for _ in 0..pending {
                    match events.try_recv() {
                        Ok(event) => record_offline_event(&mut state, &event),
                        Err(
                            broadcast::error::TryRecvError::Empty
                            | broadcast::error::TryRecvError::Closed,
                        ) => break,
                        Err(broadcast::error::TryRecvError::Lagged(skipped)) => {
                            return Err(offline_event_lag(skipped));
                        }
                    }
                }
                return Ok(state);
            }
            event = events.recv() => match event {
                Ok(event) => record_offline_event(&mut state, &event),
                Err(broadcast::error::RecvError::Closed) => return Ok(state),
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    return Err(offline_event_lag(skipped));
                }
            },
        }
    }
}

fn record_offline_event(state: &mut OfflineEventState, event: &CdpEvent) {
    let Some(session) = event.session_id.as_deref() else {
        return;
    };
    match event.method.as_ref() {
        "Network.requestWillBeSent" => {
            if let Some(url) = event.params.pointer("/request/url").and_then(Value::as_str)
                && matches!(
                    Url::parse(url).ok().as_ref().map(Url::scheme),
                    Some("http" | "https" | "ws" | "wss")
                )
            {
                state
                    .attempted_urls
                    .entry(session.to_string())
                    .or_default()
                    .record(url);
            }
        }
        "Runtime.exceptionThrown" => {
            if let Some(text) = event
                .params
                .pointer("/exceptionDetails/text")
                .and_then(Value::as_str)
            {
                push_page_error(
                    state.page_errors.entry(session.to_string()).or_default(),
                    text,
                );
            }
        }
        "Log.entryAdded" => {
            if let Some(text) = log_entry_page_error(&event.params) {
                push_page_error(
                    state.page_errors.entry(session.to_string()).or_default(),
                    text,
                );
            }
        }
        _ => {}
    }
}

fn offline_event_lag(skipped: u64) -> PageKnotError {
    PageKnotError::new(
        "pageknot.browser.cdp_event_lag",
        ErrorStage::Verification,
        format!("offline verifier fell behind by {skipped} events"),
    )
}

fn attempted_url_limit_error() -> PageKnotError {
    PageKnotError::new(
        "pageknot.verification.network",
        ErrorStage::Verification,
        "offline verification exceeded the attempted URL evidence limit",
    )
    .with_detail("maximumUrls", MAXIMUM_ATTEMPTED_URLS)
    .with_detail("maximumUrlBytes", MAXIMUM_ATTEMPTED_URL_BYTES)
}

fn push_page_error(errors: &mut Vec<String>, value: &str) {
    if errors.len() >= MAXIMUM_PAGE_ERRORS {
        return;
    }
    let mut end = value.len().min(MAXIMUM_PAGE_ERROR_BYTES);
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    errors.push(value[..end].to_owned());
}

fn log_entry_page_error(params: &Value) -> Option<&str> {
    let entry = params.get("entry")?;
    if entry.get("level").and_then(Value::as_str) != Some("error")
        || !matches!(
            entry.get("source").and_then(Value::as_str),
            Some("javascript" | "worker")
        )
    {
        return None;
    }
    entry.get("text").and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::{
        MAXIMUM_ATTEMPTED_URLS, OfflineEventState, StabilityProbe, log_entry_page_error,
        record_offline_event,
    };
    use crate::CdpEvent;

    #[test]
    fn broken_images_and_failed_fonts_prevent_stability() {
        let probe = StabilityProbe {
            ready: true,
            documents: 1,
            mutations: 0,
            pending_images: 0,
            broken_images: 1,
            pending_fonts: 0,
            failed_fonts: 1,
        };

        assert!(!probe.settled());
        assert_eq!(probe.failures("frame:child").len(), 2);
    }

    #[test]
    fn a_clean_probe_is_stable() {
        let probe = StabilityProbe {
            ready: true,
            documents: 2,
            mutations: 3,
            pending_images: 0,
            broken_images: 0,
            pending_fonts: 0,
            failed_fonts: 0,
        };

        assert!(probe.settled());
        assert!(probe.failures("main-frame").is_empty());
    }

    #[test]
    fn rendering_diagnostics_are_not_page_errors() {
        let params = json!({
            "entry": {
                "source": "rendering",
                "level": "error",
                "text": "Error: <svg> attribute height: Expected length, \"auto\"."
            }
        });

        assert_eq!(log_entry_page_error(&params), None);
    }

    #[test]
    fn javascript_and_worker_log_errors_are_page_errors() {
        for source in ["javascript", "worker"] {
            let params = json!({
                "entry": {
                    "source": source,
                    "level": "error",
                    "text": "uncaught failure"
                }
            });

            assert_eq!(log_entry_page_error(&params), Some("uncaught failure"));
        }
    }

    #[test]
    fn attempted_url_evidence_has_a_fixed_count_limit() {
        let mut state = OfflineEventState::default();
        for index in 0..=MAXIMUM_ATTEMPTED_URLS {
            let event = CdpEvent {
                method: Arc::from("Network.requestWillBeSent"),
                params: Arc::new(json!({
                    "request": {"url": format!("https://example.com/{index}")}
                })),
                session_id: Some(Arc::from("main")),
            };
            record_offline_event(&mut state, &event);
        }

        let evidence = &state.attempted_urls["main"];
        assert_eq!(evidence.urls.len(), MAXIMUM_ATTEMPTED_URLS);
        assert!(evidence.overflow);
    }
}
