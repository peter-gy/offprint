use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use pageknot_browser::AttachedFrame;
use pageknot_model::{ErrorStage, PageKnotError, Result};
use serde_json::{Value, json};
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::cdp::generated::cdp_target::{AttachedToTargetEvent, DetachedFromTargetEvent};
use crate::resources::RENDERED_RESPONSE_RESOURCE_TYPES;
use crate::{COLLECTOR_BUNDLE, CdpClient};

pub(crate) type SessionRegistry = Arc<RwLock<HashSet<String>>>;

const NETWORK_BUFFER_BYTES: u64 = 16 * 1024 * 1024;
const NETWORK_RESOURCE_BUFFER_BYTES: u64 = 512 * 1024;
const DEFAULT_MAXIMUM_ATTACHED_FRAMES: usize = 255;
const DEFAULT_MAXIMUM_MANAGED_TARGETS: usize = 1_024;

pub(crate) fn network_enable_parameters() -> Value {
    json!({
        "maxTotalBufferSize": NETWORK_BUFFER_BYTES,
        "maxResourceBufferSize": NETWORK_RESOURCE_BUFFER_BYTES,
    })
}

const CONTAINMENT_SCRIPT: &str = r#"(() => {
    const block = (name) => {
        if (!(name in globalThis)) return;
        const Blocked = function () {
            throw new DOMException("PageKnot blocked a direct network transport.", "SecurityError");
        };
        Object.defineProperty(Blocked, "name", {value: name});
        Object.defineProperty(globalThis, name, {
            configurable: false,
            enumerable: false,
            writable: false,
            value: Blocked
        });
    };
    block("RTCPeerConnection");
    block("webkitRTCPeerConnection");
})()"#;

const REMOTE_CONTAINMENT_SCRIPT: &str = r#"(() => {
    const block = (name) => {
        if (!(name in globalThis)) return;
        const Blocked = function () {
            throw new DOMException("PageKnot blocked a direct network transport.", "SecurityError");
        };
        Object.defineProperty(Blocked, "name", {value: name});
        Object.defineProperty(globalThis, name, {
            configurable: false,
            enumerable: false,
            writable: false,
            value: Blocked
        });
    };
    block("RTCPeerConnection");
    block("webkitRTCPeerConnection");
    block("WebSocket");
    block("EventSource");
})()"#;

#[derive(Clone, Copy, Debug, Default)]
struct TargetPolicy {
    fetch_enabled: bool,
    intercept_subresource_requests: bool,
    network_blocked: bool,
}

#[derive(Clone, Copy, Debug)]
enum PolicyChange {
    EnableFetch {
        intercept_subresource_requests: bool,
    },
    BlockNetwork,
}

impl PolicyChange {
    const fn committed(self, policy: TargetPolicy) -> bool {
        match self {
            Self::EnableFetch { .. } => policy.fetch_enabled,
            Self::BlockNetwork => policy.network_blocked,
        }
    }

    fn applies_to(self, target: &ManagedTarget) -> bool {
        match self {
            Self::EnableFetch { .. } => target.kind.supports_fetch_interception(),
            Self::BlockNetwork => true,
        }
    }

    async fn apply(self, client: &CdpClient, session_id: &str) -> Result<()> {
        match self {
            Self::EnableFetch {
                intercept_subresource_requests,
            } => enable_fetch(client, session_id, intercept_subresource_requests).await,
            Self::BlockNetwork => block_network(client, session_id).await,
        }
    }

    fn commit(self, policy: &mut TargetPolicy) {
        match self {
            Self::EnableFetch {
                intercept_subresource_requests,
            } => {
                policy.fetch_enabled = true;
                policy.intercept_subresource_requests = intercept_subresource_requests;
            }
            Self::BlockNetwork => policy.network_blocked = true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TargetKind {
    Document,
    ServiceWorker,
    Worker,
    Popup,
    Other,
}

impl TargetKind {
    fn from_cdp_type(target_type: &str) -> Self {
        match target_type {
            "iframe" => Self::Document,
            "service_worker" => Self::ServiceWorker,
            "worker" | "shared_worker" => Self::Worker,
            "page" => Self::Popup,
            _ => Self::Other,
        }
    }

    const fn supports_fetch_interception(self) -> bool {
        matches!(self, Self::Document | Self::ServiceWorker)
    }
}

#[derive(Clone, Debug)]
struct ManagedTarget {
    kind: TargetKind,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TargetLimits {
    maximum_frames: usize,
    maximum_targets: usize,
}

impl TargetLimits {
    pub(crate) fn for_capture(maximum_frames: u32) -> Self {
        let maximum_frames = maximum_frames.saturating_sub(1) as usize;
        let maximum_targets = maximum_frames
            .saturating_mul(4)
            .clamp(1, DEFAULT_MAXIMUM_MANAGED_TARGETS);
        Self {
            maximum_frames,
            maximum_targets,
        }
    }
}

impl Default for TargetLimits {
    fn default() -> Self {
        Self {
            maximum_frames: DEFAULT_MAXIMUM_ATTACHED_FRAMES,
            maximum_targets: DEFAULT_MAXIMUM_MANAGED_TARGETS,
        }
    }
}

#[derive(Debug, Default)]
struct TargetState {
    frames: BTreeMap<String, AttachedFrame>,
    managed: BTreeMap<String, ManagedTarget>,
    error: Option<PageKnotError>,
}

#[derive(Debug)]
pub(crate) struct FrameTargetManager {
    client: CdpClient,
    state: Arc<Mutex<TargetState>>,
    policy: Arc<RwLock<TargetPolicy>>,
    block_direct_sockets: bool,
    cancellation: CancellationToken,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

#[derive(Debug)]
struct TargetManagerRuntime {
    client: CdpClient,
    sessions: SessionRegistry,
    state: Arc<Mutex<TargetState>>,
    policy: Arc<RwLock<TargetPolicy>>,
    block_direct_sockets: bool,
    limits: TargetLimits,
    install_collector: bool,
}

impl FrameTargetManager {
    pub(crate) fn start_with_limits(
        client: CdpClient,
        main_session_id: String,
        sessions: SessionRegistry,
        block_direct_sockets: bool,
        limits: TargetLimits,
        install_collector: bool,
    ) -> Self {
        let state = Arc::new(Mutex::new(TargetState::default()));
        let policy = Arc::new(RwLock::new(TargetPolicy::default()));
        let cancellation = CancellationToken::new();
        let runtime = TargetManagerRuntime {
            client: client.clone(),
            sessions,
            state: Arc::clone(&state),
            policy: Arc::clone(&policy),
            block_direct_sockets,
            limits,
            install_collector,
        };
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            run_target_manager(runtime, main_session_id, task_cancellation).await;
        });
        Self {
            client,
            state,
            policy,
            block_direct_sockets,
            cancellation,
            task: Mutex::new(Some(task)),
        }
    }

    pub(crate) async fn enable_fetch(&self, intercept_subresource_requests: bool) -> Result<()> {
        self.apply_policy(PolicyChange::EnableFetch {
            intercept_subresource_requests,
        })
        .await
    }

    pub(crate) async fn block_network(&self) -> Result<()> {
        self.apply_policy(PolicyChange::BlockNetwork).await
    }

    async fn apply_policy(&self, change: PolicyChange) -> Result<()> {
        let mut policy = self.policy.write().await;
        let sessions = {
            let state = self.state.lock().await;
            if let Some(error) = &state.error {
                return Err(error.clone());
            }
            if change.committed(*policy) {
                return Ok(());
            }
            state
                .managed
                .iter()
                .filter(|(_, target)| change.applies_to(target))
                .map(|(session_id, _)| session_id.clone())
                .collect::<Vec<_>>()
        };
        for (applied, session_id) in sessions.into_iter().enumerate() {
            if let Err(error) = change.apply(&self.client, &session_id).await {
                if applied > 0 {
                    self.cancellation.cancel();
                    drop(policy);
                    set_target_error(&self.state, error.clone()).await;
                    self.close().await;
                }
                return Err(error);
            }
        }
        change.commit(&mut policy);
        Ok(())
    }

    pub(crate) async fn frames(&self) -> Result<Vec<AttachedFrame>> {
        self.frames_bounded(u32::MAX).await
    }

    pub(crate) async fn frames_bounded(&self, maximum: u32) -> Result<Vec<AttachedFrame>> {
        let state = self.state.lock().await;
        clone_frames_bounded(&state, maximum)
    }

    pub(crate) async fn error(&self) -> Option<PageKnotError> {
        self.state.lock().await.error.clone()
    }

    #[must_use]
    pub(crate) const fn containment_script(&self) -> &'static str {
        containment_script(self.block_direct_sockets)
    }

    pub(crate) async fn close(&self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.lock().await.take() {
            let _ignored = task.await;
        }
    }
}

fn clone_frames_bounded(state: &TargetState, maximum: u32) -> Result<Vec<AttachedFrame>> {
    if let Some(error) = &state.error {
        return Err(error.clone());
    }
    if u64::try_from(state.frames.len()).unwrap_or(u64::MAX) > u64::from(maximum) {
        return Err(frame_limit_error(maximum));
    }
    Ok(state.frames.values().cloned().collect())
}

impl Drop for FrameTargetManager {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Ok(mut task) = self.task.try_lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}

pub(crate) const fn containment_script(block_direct_sockets: bool) -> &'static str {
    if block_direct_sockets {
        REMOTE_CONTAINMENT_SCRIPT
    } else {
        CONTAINMENT_SCRIPT
    }
}

async fn run_target_manager(
    runtime: TargetManagerRuntime,
    main_session_id: String,
    cancellation: CancellationToken,
) {
    let TargetManagerRuntime {
        client,
        sessions,
        state,
        policy,
        block_direct_sockets,
        limits,
        install_collector,
    } = runtime;
    sessions.write().await.insert(main_session_id);
    let (target_event_tx, mut target_events) = mpsc::channel(limits.maximum_targets.max(1));
    let forward_task = tokio::spawn(forward_target_events(
        client.subscribe_targets(),
        target_event_tx,
        Arc::clone(&sessions),
        Arc::clone(&state),
        cancellation.clone(),
    ));
    loop {
        let event = tokio::select! {
            () = cancellation.cancelled() => {
                let _ignored = forward_task.await;
                return;
            },
            event = target_events.recv() => event,
        };
        let Some(event) = event else {
            let _ignored = forward_task.await;
            return;
        };
        let parent_session_id = event
            .session_id
            .as_deref()
            .map(str::to_owned)
            .unwrap_or_default();
        match event.method.as_ref() {
            "Target.attachedToTarget" => {
                let attached = match event.decode::<AttachedToTargetEvent>() {
                    Ok(Some(attached)) => attached,
                    Ok(None) => continue,
                    Err(error) => {
                        set_target_error(&state, error).await;
                        continue;
                    }
                };
                let kind = TargetKind::from_cdp_type(&attached.target_info.r#type);
                if kind == TargetKind::Popup {
                    let rejected = tokio::select! {
                        () = cancellation.cancelled() => return,
                        rejected = reject_popup(&client, &attached.target_info.target_id) => rejected,
                    };
                    if let Err(error) = rejected {
                        set_target_error(&state, error).await;
                    }
                    continue;
                }
                if kind == TargetKind::Other {
                    let rejected = tokio::select! {
                        () = cancellation.cancelled() => return,
                        rejected = close_target(&client, &attached.target_info.target_id) => rejected,
                    };
                    if let Err(error) = rejected {
                        set_target_error(&state, error).await;
                    }
                    continue;
                }

                let session_id = attached.session_id;
                let target_id = attached.target_info.target_id;
                let url = attached.target_info.url;
                let admitted = reserve_target(
                    &state,
                    &session_id,
                    &target_id,
                    &parent_session_id,
                    &url,
                    kind,
                    limits,
                )
                .await;
                if let Err(error) = admitted {
                    let _ignored = tokio::select! {
                        () = cancellation.cancelled() => return,
                        rejected = close_target(&client, &target_id) => rejected,
                    };
                    set_target_error(&state, error).await;
                    continue;
                }
                sessions.write().await.insert(session_id.clone());
                let target_policy = tokio::select! {
                    () = cancellation.cancelled() => return,
                    target_policy = policy.read() => target_policy,
                };
                if cancellation.is_cancelled() {
                    return;
                }
                let configured = tokio::select! {
                    () = cancellation.cancelled() => return,
                    configured = configure_target_session(
                        &client,
                        &session_id,
                        kind,
                        *target_policy,
                        block_direct_sockets,
                        install_collector,
                    ) => configured,
                };
                let resumed = if configured.is_ok() {
                    tokio::select! {
                        () = cancellation.cancelled() => return,
                        resumed = resume_target(&client, &session_id) => resumed,
                    }
                } else {
                    Ok(())
                };
                match (configured, resumed) {
                    (Ok(()), Ok(())) => {}
                    (Err(error), _) | (Ok(()), Err(error)) => {
                        sessions.write().await.remove(&session_id);
                        let _ignored = close_target(&client, &target_id).await;
                        remove_target(&state, &session_id).await;
                        set_target_error(&state, error).await;
                    }
                }
                drop(target_policy);
            }
            "Target.detachedFromTarget" => match event.decode::<DetachedFromTargetEvent>() {
                Ok(Some(detached)) => {
                    sessions.write().await.remove(&detached.session_id);
                    record_detached_target(&state, &detached.session_id).await;
                }
                Ok(None) => {}
                Err(error) => {
                    set_target_error(&state, error).await;
                }
            },
            _ => {}
        }
    }
}

async fn forward_target_events(
    mut events: broadcast::Receiver<crate::CdpEvent>,
    target_events: mpsc::Sender<crate::CdpEvent>,
    sessions: SessionRegistry,
    state: Arc<Mutex<TargetState>>,
    cancellation: CancellationToken,
) {
    loop {
        let event = tokio::select! {
            () = cancellation.cancelled() => return,
            event = events.recv() => event,
        };
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                set_target_error(
                    &state,
                    PageKnotError::new(
                        "pageknot.browser.cdp_event_lag",
                        ErrorStage::Collection,
                        format!("target event stream failed: {error}"),
                    ),
                )
                .await;
                return;
            }
        };
        if !matches!(
            event.method.as_ref(),
            "Target.attachedToTarget" | "Target.detachedFromTarget"
        ) {
            continue;
        }
        let Some(parent) = event.session_id.as_deref() else {
            continue;
        };
        if !sessions.read().await.contains(parent) {
            continue;
        }
        let sent = tokio::select! {
            () = cancellation.cancelled() => return,
            sent = target_events.send(event) => sent,
        };
        if sent.is_err() {
            return;
        }
    }
}

async fn reserve_target(
    state: &Mutex<TargetState>,
    session_id: &str,
    target_id: &str,
    parent_session_id: &str,
    url: &str,
    kind: TargetKind,
    limits: TargetLimits,
) -> Result<()> {
    let mut state = state.lock().await;
    if state.managed.len() >= limits.maximum_targets {
        return Err(PageKnotError::new(
            "pageknot.frame.limit",
            ErrorStage::Collection,
            "browser target count exceeds the configured limit",
        )
        .with_detail("limit", limits.maximum_targets));
    }
    if kind == TargetKind::Document && state.frames.len() >= limits.maximum_frames {
        return Err(frame_limit_error(
            u32::try_from(limits.maximum_frames).unwrap_or(u32::MAX),
        ));
    }
    state
        .managed
        .insert(session_id.to_owned(), ManagedTarget { kind });
    if kind == TargetKind::Document {
        state.frames.insert(
            target_id.to_owned(),
            AttachedFrame {
                session_id: session_id.to_owned(),
                target_id: target_id.to_owned(),
                parent_session_id: parent_session_id.to_owned(),
                url: url.to_owned(),
            },
        );
    }
    Ok(())
}

async fn remove_target(state: &Mutex<TargetState>, session_id: &str) {
    let mut state = state.lock().await;
    state.managed.remove(session_id);
    state
        .frames
        .retain(|_, frame| frame.session_id != session_id);
}

async fn record_detached_target(state: &Mutex<TargetState>, session_id: &str) {
    let mut state = state.lock().await;
    let kind = state.managed.remove(session_id).map(|target| target.kind);
    if kind != Some(TargetKind::Document) {
        return;
    }
    let detached = state
        .frames
        .iter()
        .find(|(_, frame)| frame.session_id == session_id)
        .map(|(target_id, _)| target_id.clone());
    if let Some(target_id) = detached {
        state.frames.remove(&target_id);
        if state.error.is_none() {
            state.error = Some(
                PageKnotError::new(
                    "pageknot.frame.detached",
                    ErrorStage::Collection,
                    "an out-of-process frame detached before collection completed",
                )
                .retryable(true),
            );
        }
    }
}

async fn configure_target_session(
    client: &CdpClient,
    session_id: &str,
    kind: TargetKind,
    policy: TargetPolicy,
    block_direct_sockets: bool,
    install_collector: bool,
) -> Result<()> {
    match kind {
        TargetKind::Document => {
            for method in ["Page.enable", "Runtime.enable", "DOM.enable", "Log.enable"] {
                client.command(method, json!({}), Some(session_id)).await?;
            }
            client
                .command(
                    "Network.enable",
                    network_enable_parameters(),
                    Some(session_id),
                )
                .await?;
            client
                .command(
                    "Page.setLifecycleEventsEnabled",
                    json!({"enabled": true}),
                    Some(session_id),
                )
                .await?;
            client
                .command(
                    "Page.addScriptToEvaluateOnNewDocument",
                    json!({
                        "source": containment_script(block_direct_sockets),
                        "runImmediately": true
                    }),
                    Some(session_id),
                )
                .await?;
            if install_collector {
                client
                    .command(
                        "Page.addScriptToEvaluateOnNewDocument",
                        json!({"source": COLLECTOR_BUNDLE, "runImmediately": true}),
                        Some(session_id),
                    )
                    .await?;
            }
        }
        TargetKind::ServiceWorker => {
            if policy.fetch_enabled {
                enable_fetch(client, session_id, policy.intercept_subresource_requests).await?;
            }
            return Ok(());
        }
        TargetKind::Worker => {
            client
                .command("Runtime.enable", json!({}), Some(session_id))
                .await?;
            client
                .command(
                    "Network.enable",
                    network_enable_parameters(),
                    Some(session_id),
                )
                .await?;
            client
                .command(
                    "Runtime.evaluate",
                    json!({
                        "expression": containment_script(block_direct_sockets),
                        "awaitPromise": false,
                        "returnByValue": true
                    }),
                    Some(session_id),
                )
                .await?;
        }
        TargetKind::Popup | TargetKind::Other => return Ok(()),
    }
    client
        .command(
            "Target.setAutoAttach",
            auto_attach_parameters(),
            Some(session_id),
        )
        .await?;
    if policy.fetch_enabled && kind.supports_fetch_interception() {
        enable_fetch(client, session_id, policy.intercept_subresource_requests).await?;
    }
    if policy.network_blocked {
        block_network(client, session_id).await?;
    }
    Ok(())
}

pub(crate) fn auto_attach_parameters() -> Value {
    json!({
        "autoAttach": true,
        "waitForDebuggerOnStart": true,
        "flatten": true,
        "filter": [
            {"type": "page", "exclude": false},
            {"type": "iframe", "exclude": false},
            {"type": "worker", "exclude": false},
            {"type": "service_worker", "exclude": false},
            {"type": "shared_worker", "exclude": false}
        ],
    })
}

pub(crate) async fn enable_fetch(
    client: &CdpClient,
    session_id: &str,
    intercept_subresource_requests: bool,
) -> Result<()> {
    client
        .command(
            "Fetch.enable",
            fetch_enable_parameters(intercept_subresource_requests),
            Some(session_id),
        )
        .await
        .map(|_| ())
}

pub(crate) fn fetch_enable_parameters(intercept_subresource_requests: bool) -> Value {
    let request_type = (!intercept_subresource_requests).then_some("Document");
    let mut patterns = ["http://*", "https://*"]
        .into_iter()
        .map(|url_pattern| {
            let mut pattern = json!({
                "urlPattern": url_pattern,
                "requestStage": "Request"
            });
            if let Some(request_type) = request_type {
                pattern["resourceType"] = Value::String(request_type.to_owned());
            }
            pattern
        })
        .collect::<Vec<_>>();
    // Chromium's Fetch domain rejects TextTrack filters, so text tracks use
    // the post-load resource path.
    for resource_type in RENDERED_RESPONSE_RESOURCE_TYPES
        .iter()
        .copied()
        .filter(|resource_type| *resource_type != "TextTrack")
    {
        patterns.push(json!({
            "urlPattern": "http://*",
            "resourceType": resource_type,
            "requestStage": "Response"
        }));
        patterns.push(json!({
            "urlPattern": "https://*",
            "resourceType": resource_type,
            "requestStage": "Response"
        }));
    }
    json!({
        "patterns": patterns,
        "handleAuthRequests": false,
    })
}

pub(crate) async fn block_network(client: &CdpClient, session_id: &str) -> Result<()> {
    client
        .command(
            "Network.setBlockedURLs",
            json!({"urls": ["http://*", "https://*", "ws://*", "wss://*"]}),
            Some(session_id),
        )
        .await
        .map(|_| ())
}

async fn resume_target(client: &CdpClient, session_id: &str) -> Result<()> {
    client
        .command_no_wait(
            "Runtime.runIfWaitingForDebugger",
            json!({}),
            Some(session_id),
        )
        .await
}

async fn reject_popup(client: &CdpClient, target_id: &str) -> Result<()> {
    close_target(client, target_id).await.map_err(|error| {
        PageKnotError::new(
            "pageknot.navigation.popup",
            ErrorStage::Navigation,
            "failed to reject a popup target before it resumed",
        )
        .with_detail("cause", error.code.as_str())
    })
}

async fn close_target(client: &CdpClient, target_id: &str) -> Result<()> {
    client
        .command("Target.closeTarget", json!({"targetId": target_id}), None)
        .await
        .map(|_| ())
}

fn frame_limit_error(maximum: u32) -> PageKnotError {
    PageKnotError::new(
        "pageknot.frame.limit",
        ErrorStage::Collection,
        "frame count exceeds the configured limit",
    )
    .with_detail("limit", maximum)
}

async fn set_target_error(state: &Mutex<TargetState>, error: PageKnotError) {
    let mut state = state.lock().await;
    if state.error.is_none() {
        state.error = Some(error);
    }
}

#[cfg(test)]
mod tests;
