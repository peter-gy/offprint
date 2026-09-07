use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use offprint_browser::NetworkGuard;
use offprint_model::{ErrorStage, OffprintError, RequestHeader, Result};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::{continued_headers, is_long_lived_response, same_origin, validate_guard_destination};
use crate::CdpClient;
use crate::resources::{
    CapturedInterceptedResponse, InterceptedResponse, ObservedResourceRecorder,
    captures_rendered_response,
};

const DNS_DECISION_TIMEOUT: Duration = Duration::from_secs(5);
const MAXIMUM_DNS_DECISIONS: usize = 32;
const MAXIMUM_RESPONSE_STREAMS: usize = 4;

#[derive(Clone)]
pub(super) struct InterceptionContext {
    client: CdpClient,
    observed_resources: ObservedResourceRecorder,
    guard: Arc<NetworkGuard>,
    headers: Arc<Vec<RequestHeader>>,
    header_origin: Arc<Url>,
    cancellation: CancellationToken,
    dns_decisions: Arc<Semaphore>,
    response_streams: Arc<Semaphore>,
}

impl InterceptionContext {
    pub(super) fn new(
        client: CdpClient,
        observed_resources: ObservedResourceRecorder,
        guard: NetworkGuard,
        headers: Vec<RequestHeader>,
        header_origin: Url,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            client,
            observed_resources,
            guard: Arc::new(guard),
            headers: Arc::new(headers),
            header_origin: Arc::new(header_origin),
            cancellation,
            dns_decisions: Arc::new(Semaphore::new(MAXIMUM_DNS_DECISIONS)),
            response_streams: Arc::new(Semaphore::new(MAXIMUM_RESPONSE_STREAMS)),
        }
    }
}

#[derive(Debug)]
pub(super) struct InterceptionFailure {
    pub(super) error: OffprintError,
    pub(super) frame_id: Option<String>,
    pub(super) document_request: bool,
}

#[derive(Debug)]
pub(super) struct PausedRequest {
    session_id: String,
    request_id: String,
    parameters: Arc<Value>,
}

impl PausedRequest {
    pub(super) fn from_event(session_id: &str, parameters: Arc<Value>) -> Result<Self> {
        let request_id = parameters
            .get("requestId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                OffprintError::new(
                    "offprint.browser.cdp_shape",
                    ErrorStage::Navigation,
                    "Fetch.requestPaused omitted requestId",
                )
            })?;
        Ok(Self {
            session_id: session_id.to_owned(),
            request_id: request_id.to_owned(),
            parameters,
        })
    }

    pub(super) async fn run(self, context: InterceptionContext) -> Vec<InterceptionFailure> {
        let prepared = self.prepare(&context).await;
        let (resolution, mut failures) = match prepared {
            PreparedRequest::Resolve(resolution) => (resolution, Vec::new()),
            PreparedRequest::ResolveWithFailure {
                resolution,
                failure,
            } => (resolution, vec![failure]),
            PreparedRequest::Capture {
                network_id,
                response_code,
                frame_id,
            } => {
                let captured = tokio::select! {
                    () = context.cancellation.cancelled() => None,
                    captured = capture_response(
                        &context,
                        &self,
                        &network_id,
                    ) => Some(captured),
                };
                match captured {
                    Some(Ok(captured)) => (
                        TerminalResolution::Fulfill {
                            response_code,
                            captured,
                        },
                        Vec::new(),
                    ),
                    Some(Err(error)) => {
                        context
                            .observed_resources
                            .reject_intercepted_response(&self.session_id, &network_id)
                            .await;
                        (
                            TerminalResolution::Fail {
                                error_reason: "Failed",
                            },
                            vec![InterceptionFailure {
                                error,
                                frame_id,
                                document_request: false,
                            }],
                        )
                    }
                    None => {
                        context
                            .observed_resources
                            .reject_intercepted_response(&self.session_id, &network_id)
                            .await;
                        (
                            TerminalResolution::Fail {
                                error_reason: "Failed",
                            },
                            Vec::new(),
                        )
                    }
                }
            }
            PreparedRequest::Guard {
                destination,
                inject_headers,
                frame_id,
                document_request,
            } => {
                let decision = tokio::select! {
                    () = context.cancellation.cancelled() => None,
                    decision = bounded_guard_destination(
                        &context,
                        &destination,
                        DNS_DECISION_TIMEOUT,
                    ) => Some(decision),
                };
                match decision {
                    Some(Ok(())) => (
                        TerminalResolution::ContinueRequest {
                            headers: (inject_headers && !context.headers.is_empty()).then(|| {
                                continued_headers(&self.parameters, context.headers.as_slice())
                            }),
                        },
                        Vec::new(),
                    ),
                    Some(Err(error)) => (
                        TerminalResolution::Fail {
                            error_reason: "BlockedByClient",
                        },
                        vec![InterceptionFailure {
                            error,
                            frame_id,
                            document_request,
                        }],
                    ),
                    None => (
                        TerminalResolution::Fail {
                            error_reason: "Failed",
                        },
                        Vec::new(),
                    ),
                }
            }
        };
        if let Err(error) = self.resolve(&context.client, resolution).await {
            failures.push(InterceptionFailure {
                error,
                frame_id: None,
                document_request: true,
            });
        }
        failures
    }

    pub(super) async fn fail_overloaded(self, client: &CdpClient) -> Result<()> {
        let session_id = self.session_id.clone();
        let command = self.terminal_command(TerminalResolution::Fail {
            error_reason: "Failed",
        });
        client
            .command_no_wait(command.method, command.parameters, Some(&session_id))
            .await
    }

    async fn prepare(&self, context: &InterceptionContext) -> PreparedRequest {
        if self.parameters.get("responseStatusCode").is_some() {
            return self.prepare_response(context).await;
        }
        self.prepare_request(context)
    }

    async fn prepare_response(&self, context: &InterceptionContext) -> PreparedRequest {
        let response_code = self
            .parameters
            .get("responseStatusCode")
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(200);
        let continue_without_body = !captures_rendered_response(
            self.parameters.get("resourceType").and_then(Value::as_str),
        ) || is_long_lived_response(&self.parameters)
            || matches!(response_code, 204 | 205 | 304)
            || (300..400).contains(&response_code);
        if continue_without_body {
            return PreparedRequest::Resolve(TerminalResolution::ContinueResponse);
        }
        let Some(network_id) = self
            .parameters
            .get("networkId")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return PreparedRequest::Resolve(TerminalResolution::ContinueResponse);
        };
        if !context
            .observed_resources
            .begin_stream(&self.session_id, &network_id, &self.parameters)
            .await
        {
            return PreparedRequest::Resolve(TerminalResolution::ContinueResponse);
        }
        PreparedRequest::Capture {
            network_id,
            response_code,
            frame_id: self.frame_id(),
        }
    }

    fn prepare_request(&self, context: &InterceptionContext) -> PreparedRequest {
        let destination = self
            .parameters
            .pointer("/request/url")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                OffprintError::new(
                    "offprint.browser.cdp_shape",
                    ErrorStage::Navigation,
                    "Fetch.requestPaused omitted the request URL",
                )
            })
            .and_then(|value| {
                Url::parse(value).map_err(|error| {
                    OffprintError::new(
                        "offprint.navigation.url",
                        ErrorStage::Navigation,
                        format!("Chromium requested an invalid URL: {error}"),
                    )
                })
            });
        let frame_id = self.frame_id();
        let document_request = self.document_request();
        match destination {
            Ok(destination) => PreparedRequest::Guard {
                inject_headers: same_origin(&destination, &context.header_origin),
                destination,
                frame_id,
                document_request,
            },
            Err(error) => PreparedRequest::ResolveWithFailure {
                resolution: TerminalResolution::Fail {
                    error_reason: "BlockedByClient",
                },
                failure: InterceptionFailure {
                    error,
                    frame_id,
                    document_request,
                },
            },
        }
    }

    async fn resolve(self, client: &CdpClient, resolution: TerminalResolution) -> Result<()> {
        let session_id = self.session_id.clone();
        let command = self.terminal_command(resolution);
        client
            .command(command.method, command.parameters, Some(&session_id))
            .await
            .map(|_| ())
    }

    fn terminal_command(self, resolution: TerminalResolution) -> TerminalCommand {
        let mut parameters = json!({"requestId": self.request_id});
        let method = match resolution {
            TerminalResolution::ContinueRequest { headers } => {
                if let Some(headers) = headers {
                    parameters["headers"] = Value::Array(headers);
                }
                "Fetch.continueRequest"
            }
            TerminalResolution::ContinueResponse => "Fetch.continueResponse",
            TerminalResolution::Fulfill {
                response_code,
                captured,
            } => {
                parameters["responseCode"] = Value::from(response_code);
                parameters["responseHeaders"] = Value::Array(captured.response_headers);
                parameters["body"] = Value::String(captured.body);
                "Fetch.fulfillRequest"
            }
            TerminalResolution::Fail { error_reason } => {
                parameters["errorReason"] = Value::String(error_reason.to_owned());
                "Fetch.failRequest"
            }
        };
        TerminalCommand { method, parameters }
    }

    fn frame_id(&self) -> Option<String> {
        self.parameters
            .get("frameId")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    fn document_request(&self) -> bool {
        self.parameters.get("resourceType").and_then(Value::as_str) == Some("Document")
            || self
                .parameters
                .get("isNavigationRequest")
                .and_then(Value::as_bool)
                .unwrap_or(false)
    }
}

enum PreparedRequest {
    Resolve(TerminalResolution),
    ResolveWithFailure {
        resolution: TerminalResolution,
        failure: InterceptionFailure,
    },
    Capture {
        network_id: String,
        response_code: u16,
        frame_id: Option<String>,
    },
    Guard {
        destination: Url,
        inject_headers: bool,
        frame_id: Option<String>,
        document_request: bool,
    },
}

enum TerminalResolution {
    ContinueRequest {
        headers: Option<Vec<Value>>,
    },
    ContinueResponse,
    Fulfill {
        response_code: u16,
        captured: CapturedInterceptedResponse,
    },
    Fail {
        error_reason: &'static str,
    },
}

struct TerminalCommand {
    method: &'static str,
    parameters: Value,
}

async fn capture_response(
    context: &InterceptionContext,
    request: &PausedRequest,
    network_id: &str,
) -> Result<CapturedInterceptedResponse> {
    let _permit = context
        .response_streams
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| {
            OffprintError::new(
                "offprint.browser.interception_closed",
                ErrorStage::Resource,
                "response interception closed before the body was captured",
            )
        })?;
    context
        .observed_resources
        .capture_intercepted_response(
            &context.client,
            InterceptedResponse {
                session_id: &request.session_id,
                request_id: &request.request_id,
                network_id,
                parameters: &request.parameters,
                cancellation: context.cancellation.clone(),
            },
        )
        .await
}

async fn bounded_guard_destination(
    context: &InterceptionContext,
    destination: &Url,
    deadline: Duration,
) -> Result<()> {
    decision_with_deadline(deadline, async {
        let _permit = context.dns_decisions.acquire().await.map_err(|_| {
            OffprintError::new(
                "offprint.browser.interception_closed",
                ErrorStage::Navigation,
                "network policy decisions are closed",
            )
        })?;
        validate_guard_destination(&context.guard, destination).await
    })
    .await
}

async fn decision_with_deadline<F>(deadline: Duration, decision: F) -> Result<()>
where
    F: Future<Output = Result<()>>,
{
    timeout(deadline, decision)
        .await
        .map_err(|_| dns_timeout_error())?
}

fn dns_timeout_error() -> OffprintError {
    OffprintError::new(
        "offprint.navigation.dns_timeout",
        ErrorStage::Navigation,
        "network policy DNS decision exceeded its deadline",
    )
    .retryable(true)
}

#[cfg(test)]
mod tests {
    use std::future::pending;

    use super::*;

    #[tokio::test]
    async fn decision_deadline_bounds_semaphore_and_dns_work() {
        let result =
            decision_with_deadline(Duration::from_millis(20), pending::<Result<()>>()).await;

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.navigation.dns_timeout")
        );
        assert_eq!(
            dns_timeout_error().code.as_str(),
            "offprint.navigation.dns_timeout"
        );
    }
}
