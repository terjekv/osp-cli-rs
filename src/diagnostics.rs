//! Command-local, allowlisted service evidence. No payloads or headers are accepted.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::core::MessageLevel;

/// Reviewed service evidence submitted by an adapter.
#[derive(Debug, Clone)]
pub enum DiagnosticEvent {
    /// Server operations not yet exposed by the curated command catalog.
    CatalogGap {
        /// Human service name.
        service: &'static str,
        /// HTTP methods and path templates, never request values.
        operations: Vec<String>,
    },
    /// First use of a configured service and its credential binding.
    Context {
        /// Human service name.
        service: &'static str,
        /// Service URL; userinfo, query and fragment are removed.
        url: String,
        /// Effective account, when applicable.
        identity: Option<String>,
        /// Active profile.
        profile: String,
        /// Credential provenance, never its value.
        credential: CredentialSource,
    },
    /// One actual transport attempt, including retries.
    Exchange {
        /// HTTP method or LDAP operation.
        method: &'static str,
        /// Destination without secret URL components.
        url: String,
        /// HTTP status, absent for transport failures.
        status: Option<u16>,
        /// Reviewed transport failure class when no response arrived.
        failure: Option<TransportFailure>,
        /// Time until response headers or transport failure.
        elapsed_ms: u128,
        /// Server-supplied correlation identifier.
        request_id: Option<String>,
        /// Declared response length, when available.
        bytes: Option<u64>,
        /// Allowlisted response media type.
        content_type: Option<&'static str>,
    },
    /// One completed directory search; filter and bind values are excluded.
    DirectorySearch {
        /// Directory search base, without the bind identity.
        base: String,
        /// Result of this logical directory request.
        outcome: DirectoryOutcome,
        /// Number of requested attributes.
        attributes: usize,
        /// Search elapsed time.
        elapsed_ms: u128,
    },
    /// An executable's final outcome and reviewed scalar details.
    PluginOutcome {
        /// Discovered command name.
        plugin: String,
        /// Actual process exit status.
        exit_code: i32,
        /// Measured child execution time.
        elapsed_ms: u128,
        /// Validated application failure code, when supplied.
        error_code: Option<String>,
        /// Reported attempt count, when numeric.
        attempt: Option<u64>,
        /// Backend identifier, accepted only as a bounded lowercase symbol.
        backend: Option<String>,
        /// Captured stdout byte count, when captured.
        bytes: Option<usize>,
    },
    /// Reviewed request policy following an actual attempt.
    RequestFacts {
        /// Configured deadline, when known.
        deadline_seconds: Option<u64>,
        /// Allowlisted projection name, never an arbitrary query value.
        projection: Option<&'static str>,
        /// Whether this request used a page continuation.
        cursor_page: bool,
    },
    /// Distinct response correlation fields supplied by the service.
    RequestIds {
        /// Header request ID.
        header: String,
        /// Body request ID.
        body: String,
    },
    /// Outcome of bounded credential recovery after a rejection.
    Recovery {
        /// Whether a replacement credential was available for a read retry.
        retry_available: bool,
    },
    /// Local cache observation.
    Cache {
        /// Stable cache name owned by the adapter.
        name: &'static str,
        /// Whether the cached value was used.
        hit: bool,
        /// Measured age in seconds.
        age_seconds: Option<u64>,
        /// Configured lifetime in seconds.
        ttl_seconds: Option<u64>,
    },
}

/// Allowlisted transport outcomes, without dependency error prose.
#[derive(Debug, Clone, Copy)]
pub enum TransportFailure {
    /// The listener refused the connection.
    ConnectionRefused,
    /// The request exceeded its deadline.
    Timeout,
    /// Server trust verification failed.
    Tls,
    /// Another transport failure without a more precise reviewed class.
    Other,
}

/// Safe outcome of a directory request; server prose and bind data are excluded.
#[derive(Debug, Clone, Copy)]
pub enum DirectoryOutcome {
    /// A search returned this many entries.
    Success(usize),
    /// The remote TCP listener refused the connection.
    ConnectionRefused,
    /// Connection setup failed before a search could run.
    ConnectionFailed,
}

/// Credential provenance accepted by diagnostics.
#[derive(Debug, Clone, Copy)]
pub enum CredentialSource {
    /// Persisted authenticated session.
    SavedSession,
    /// No local session.
    NoSession,
    /// Locally known expired session.
    ExpiredSession,
    /// Explicit password acquisition.
    PasswordLogin,
    /// Directory service binding.
    ServiceBind,
    /// Anonymous directory reads without a bind credential.
    Anonymous,
    /// Configured API key.
    ApiKey,
    /// Missing API key.
    NoApiKey,
}

impl CredentialSource {
    fn label(self) -> &'static str {
        match self {
            Self::SavedSession => "saved session",
            Self::NoSession => "no session",
            Self::ExpiredSession => "expired session",
            Self::PasswordLogin => "password login",
            Self::ServiceBind => "service bind",
            Self::Anonymous => "anonymous bind",
            Self::ApiKey => "API key",
            Self::NoApiKey => "no API key",
        }
    }
}

/// Removes URL userinfo, query values and fragments before storage or display.
pub fn safe_url(raw: &str) -> String {
    let raw = raw.split(['?', '#']).next().unwrap_or_default();
    let Some((scheme, rest)) = raw.split_once("://") else {
        return "<invalid URL>".into();
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let authority = authority.rsplit('@').next().unwrap_or_default();
    safe_text(&format!(
        "{scheme}://{authority}{}",
        if path.is_empty() {
            String::new()
        } else {
            format!("/{path}")
        }
    ))
}

/// Escapes terminal controls and bounds a reviewed single-line field.
pub(crate) fn safe_text(raw: &str) -> String {
    raw.chars().take(240).flat_map(char::escape_debug).collect()
}

/// Bounded sanitized evidence for one command, shared with native adapters.
#[derive(Debug, Clone)]
pub struct DiagnosticSink {
    started: Instant,
    state: Arc<Mutex<Evidence>>,
}

#[derive(Debug, Default)]
struct Evidence {
    lines: Vec<(MessageLevel, String)>,
    services: Vec<&'static str>,
    requests: usize,
}

impl Default for DiagnosticSink {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            state: Arc::default(),
        }
    }
}

impl DiagnosticSink {
    /// Accepts only reviewed event fields; sanitizes before retaining anything.
    pub fn emit(&self, event: DiagnosticEvent) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Evidence {
            lines,
            services,
            requests,
        } = &mut *state;
        let first = lines.len();
        let event_name = match &event {
            DiagnosticEvent::Context { .. } => "service.context",
            DiagnosticEvent::CatalogGap { .. } => "service.catalog_gap",
            DiagnosticEvent::Exchange { .. } => "service.exchange",
            DiagnosticEvent::DirectorySearch { .. } => "directory.search",
            DiagnosticEvent::PluginOutcome { .. } => "plugin.outcome",
            DiagnosticEvent::Cache { .. } => "cache.lookup",
            DiagnosticEvent::RequestFacts { .. } => "request.policy",
            DiagnosticEvent::RequestIds { .. } => "response.request_ids",
            DiagnosticEvent::Recovery { .. } => "session.recovery",
        };
        let mut insert = |level, text| lines.push((level, text));
        match event {
            DiagnosticEvent::CatalogGap {
                service,
                operations,
            } => {
                let message = format!(
                    "info: {} offers {} {} osp does not know yet ({}).\n",
                    safe_text(service),
                    operations.len(),
                    if operations.len() == 1 {
                        "operation"
                    } else {
                        "operations"
                    },
                    operations
                        .iter()
                        .map(|operation| safe_text(operation))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                tracing::info!(target: "osp::diagnostic", "{}", message.trim_end());
                insert(MessageLevel::Info, message);
            }
            DiagnosticEvent::Context {
                service,
                url,
                identity,
                profile,
                credential,
            } => {
                if services.contains(&service) {
                    return;
                }
                services.push(service);
                let account = identity
                    .map(|name| format!(" as {}", safe_text(&name)))
                    .unwrap_or_default();
                insert(
                    MessageLevel::Info,
                    format!(
                        "info: {service} at {}{account} (profile {}, {}).\n",
                        safe_url(&url),
                        safe_text(&profile),
                        credential.label()
                    ),
                );
            }
            DiagnosticEvent::Exchange {
                method,
                url,
                status,
                failure,
                elapsed_ms,
                request_id,
                bytes,
                content_type,
            } => {
                *requests += 1;
                let response = match (status, failure) {
                    (Some(status), _) => format!(
                        "HTTP {status} in {elapsed_ms} ms{}.",
                        request_id
                            .map(|id| format!(" (request {})", safe_text(&id)))
                            .unwrap_or_default()
                    ),
                    (_, Some(TransportFailure::ConnectionRefused)) => "Connection refused.".into(),
                    (_, Some(TransportFailure::Timeout)) => {
                        format!("Timed out after {elapsed_ms} ms.")
                    }
                    (_, Some(TransportFailure::Tls)) => "TLS verification failed.".into(),
                    _ => format!("Transport failure in {elapsed_ms} ms."),
                };
                insert(
                    MessageLevel::Trace,
                    format!(
                        "request: {method} {}\nresponse: {response}\n",
                        safe_url(&url)
                    ),
                );
                if let Some(failure) = failure {
                    let failure = match failure {
                        TransportFailure::ConnectionRefused => "connection-refused",
                        TransportFailure::Timeout => "timeout",
                        TransportFailure::Tls => "tls-verification",
                        TransportFailure::Other => "transport",
                    };
                    insert(
                        MessageLevel::Evidence,
                        format!("evidence: failure={failure}; elapsed={elapsed_ms} ms.\n"),
                    );
                }
                if let Some(content_type) = content_type {
                    insert(
                        MessageLevel::Evidence,
                        format!("evidence: content-type={content_type}.\n"),
                    );
                }
                if let Some(bytes) = bytes {
                    insert(
                        MessageLevel::Evidence,
                        format!("evidence: response-bytes={bytes}.\n"),
                    );
                }
            }
            DiagnosticEvent::DirectorySearch {
                base,
                outcome,
                attributes,
                elapsed_ms,
            } => {
                *requests += 1;
                let response = match outcome {
                    DirectoryOutcome::Success(entries) => format!(
                        "LDAP success; {entries} {} in {elapsed_ms} ms.",
                        if entries == 1 { "entry" } else { "entries" }
                    ),
                    DirectoryOutcome::ConnectionRefused => {
                        "Connection refused before search.".into()
                    }
                    DirectoryOutcome::ConnectionFailed => "Connection failed before search.".into(),
                };
                insert(
                    MessageLevel::Trace,
                    format!(
                        "request: LDAP search {}\nresponse: {response}\n",
                        safe_text(&base)
                    ),
                );
                insert(
                    MessageLevel::Evidence,
                    format!("evidence: projection-attributes={attributes}.\n"),
                );
            }
            DiagnosticEvent::PluginOutcome {
                plugin,
                exit_code,
                elapsed_ms,
                error_code,
                attempt,
                backend,
                bytes,
            } => {
                if let Some(backend) = backend {
                    insert(
                        MessageLevel::Info,
                        format!("detail: backend={}\n", safe_text(&backend)),
                    );
                }
                if let Some(attempt) = attempt {
                    insert(MessageLevel::Info, format!("detail: attempt={attempt}\n"));
                }
                let outcome = if let Some(code) = error_code {
                    insert(
                        MessageLevel::Info,
                        format!("detail: code={}\n", safe_text(&code)),
                    );
                    format!("returned {}", safe_text(&code))
                } else if exit_code != 0 {
                    format!("exited with status {exit_code}")
                } else {
                    "completed".into()
                };
                insert(
                    MessageLevel::Trace,
                    format!(
                        "response: Plugin {} {outcome} in {elapsed_ms} ms.\n",
                        safe_text(&plugin)
                    ),
                );
                if let Some(bytes) = bytes {
                    insert(
                        MessageLevel::Evidence,
                        format!("evidence: Plugin stdout contains {bytes} bytes.\n"),
                    );
                }
            }
            DiagnosticEvent::RequestFacts {
                deadline_seconds,
                projection,
                cursor_page,
            } => {
                if cursor_page {
                    insert(MessageLevel::Trace, "request: Page continuation.\n".into());
                }
                if let Some(seconds) = deadline_seconds {
                    insert(
                        MessageLevel::Evidence,
                        format!("evidence: deadline={seconds}s.\n"),
                    );
                }
                if let Some(projection) = projection {
                    insert(
                        MessageLevel::Evidence,
                        format!("evidence: query.view={projection}.\n"),
                    );
                }
            }
            DiagnosticEvent::RequestIds { header, body } => insert(
                MessageLevel::Evidence,
                format!(
                    "evidence: Header request-id={}; body request-id={}.\n",
                    safe_text(&header),
                    safe_text(&body)
                ),
            ),
            DiagnosticEvent::Recovery { retry_available } => {
                if retry_available {
                    insert(
                        MessageLevel::Trace,
                        "request: Retrying after session renewal.\n".into(),
                    );
                }
                insert(
                    MessageLevel::Evidence,
                    format!(
                        "evidence: Rejected session invalidated; replacement credential {}.\n",
                        if retry_available {
                            "available"
                        } else {
                            "unavailable"
                        }
                    ),
                );
            }
            DiagnosticEvent::Cache {
                name,
                hit,
                age_seconds,
                ttl_seconds,
            } => {
                insert(
                    MessageLevel::Trace,
                    format!(
                        "cache: {name} {}.\n",
                        if hit {
                            if age_seconds
                                .zip(ttl_seconds)
                                .is_some_and(|(age, ttl)| age > ttl)
                            {
                                "stale; used"
                            } else {
                                "hit; used"
                            }
                        } else {
                            "miss"
                        }
                    ),
                );
                let mut facts = Vec::new();
                if let Some(age) = age_seconds {
                    facts.push(format!("age={age}s"));
                }
                if let Some(ttl) = ttl_seconds {
                    facts.push(format!("TTL={ttl}s"));
                }
                if !facts.is_empty() {
                    insert(
                        MessageLevel::Evidence,
                        format!("evidence: {name} {}.\n", facts.join("; ")),
                    );
                }
            }
        }
        lines.truncate(256);
        for (_, line) in &lines[first..] {
            tracing::info!(target: "osp::evidence", diagnostic_event = event_name, message = %line.trim_end());
        }
    }

    pub(crate) fn render(&self, level: MessageLevel, failed: bool) -> (String, String) {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let before = state
            .lines
            .iter()
            .filter(|(minimum, _)| level >= *minimum)
            .map(|(_, line)| line.as_str())
            .collect();
        let after = if level >= MessageLevel::Info && !state.services.is_empty() {
            let count = state.requests;
            let requests = if count == 0 {
                String::new()
            } else {
                format!(" ({count} request{})", if count == 1 { "" } else { "s" })
            };
            format!(
                "info: {} in {} ms{requests}.\n",
                if failed { "Failed" } else { "Done" },
                self.started.elapsed().as_millis()
            )
        } else {
            String::new()
        };
        (before, after)
    }
}

/// One local-only setup observation supplied by the product.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalCheck {
    /// One of `ok`, `warning` or `info`.
    pub status: &'static str,
    /// Reviewed operator text without credentials or remote health claims.
    pub message: String,
    /// Optional single next action.
    pub action: Option<String>,
    /// Additional local evidence shown with verbosity.
    pub detail: Option<String>,
    /// Bound service name, when this row describes a credential.
    pub service: Option<&'static str>,
    /// Safe service URL, without credentials or query values.
    pub url: Option<String>,
    /// Selected identity, when relevant.
    pub identity: Option<String>,
    /// Bound profile, when relevant.
    pub profile: Option<String>,
    /// Local credential state; presence does not imply server acceptance.
    pub session: Option<&'static str>,
    /// Locally known expiry as Unix seconds.
    pub expires_at: Option<u64>,
}

thread_local! {
    static ACTIVE: std::cell::RefCell<Option<DiagnosticSink>> = const { std::cell::RefCell::new(None) };
}

pub(crate) struct DiagnosticScope(Option<DiagnosticSink>);
impl Drop for DiagnosticScope {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.replace(self.0.take()));
    }
}

impl DiagnosticSink {
    /// Returns the evidence sink active on this synchronous command thread.
    pub fn current() -> Option<Self> {
        ACTIVE.with(|active| active.borrow().clone())
    }

    /// Shares command-local evidence with a bounded worker operation.
    pub fn scope<T>(&self, operation: impl FnOnce() -> T) -> T {
        let _scope = self.enter();
        operation()
    }

    pub(crate) fn enter(&self) -> DiagnosticScope {
        DiagnosticScope(ACTIVE.with(|active| active.replace(Some(self.clone()))))
    }
}

/// Submits evidence from a synchronous adapter in the current command scope.
/// Background work outside that scope produces no command diagnostics.
pub fn emit(event: DiagnosticEvent) {
    ACTIVE.with(|active| {
        if let Some(sink) = active.borrow().as_ref() {
            sink.emit(event);
        }
    });
}
