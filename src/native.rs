//! In-process native command surface.
//!
//! This module exists so `osp` can expose built-in commands through the same
//! catalog, policy, and dispatch-adjacent shapes that plugin commands use,
//! without spawning a subprocess.
//!
//! High-level flow:
//!
//! - register native command implementations in a [`NativeCommandRegistry`]
//! - describe them through clap-derived metadata
//! - project that metadata into completion, help, and policy surfaces
//! - execute them in-process with a small runtime context
//!
//! Contract:
//!
//! - native commands are the in-process counterpart to plugin commands
//! - catalog and policy projection should stay aligned with the plugin-facing
//!   protocol types in `crate::core::plugin`
//!
//! Public API shape:
//!
//! - [`NativeCommandRegistry`] is the canonical registration surface
//! - catalog/context structs stay plain describe-time or execute-time payloads
//! - commands should expose behavior through the registry rather than by
//!   leaking host-internal runtime state
//! - downstream product crates typically build a registry once and pass it
//!   into [`crate::App::with_native_commands`] or
//!   [`crate::App::with_native_commands`] as part of their own wrapper
//!   or builder layer

use std::collections::BTreeMap;
use std::fmt;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use anyhow::Result;
use clap::Command;

use crate::completion::CompletionNode;
use crate::config::ResolvedConfig;
use crate::core::command_policy::CommandPath;
use crate::core::plugin::{
    DescribeCommandAuthV1, DescribeCommandV1, ResponseMessageV1, ResponseMetaV1, ResponseV1,
};
use crate::core::runtime::RuntimeHints;
use crate::plugin::catalog::collect_command_auth;
use crate::plugin::{CommandCatalogEntry, CommandSource};

/// Elevation requested for one native-command execution.
///
/// The host only transports this intent. Product wrappers own credential
/// acquisition and decide which server request, if any, uses the elevated
/// credential.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InvocationElevation {
    /// Use the ordinary session credential.
    #[default]
    Ordinary,
    /// Use the product-owned elevated credential for this invocation.
    Elevated,
}

impl InvocationElevation {
    /// Returns whether this invocation explicitly requested elevation.
    pub fn is_elevated(self) -> bool {
        matches!(self, Self::Elevated)
    }
}

/// Runtime context passed to native command implementations.
///
/// This keeps the command surface small and stable: commands receive the
/// resolved config snapshot and runtime hints they need to behave like the host
/// would, without exposing the whole app runtime for ad hoc coupling.
#[non_exhaustive]
pub struct NativeCommandContext<'a> {
    /// Current resolved config snapshot for this execution.
    pub config: &'a ResolvedConfig,
    /// Runtime hints that should be propagated to child processes and adapters.
    pub runtime_hints: RuntimeHints,
    /// Explicit elevation intent for this execution.
    pub invocation_elevation: InvocationElevation,
    /// Session-scoped native context that may be surfaced by the host REPL.
    pub session_context: NativeSessionContext,
    /// Structured command hints and history entries for the hosting REPL.
    pub repl_feedback: crate::repl::ReplFeedback,
    command_scope: crate::core::CommandScope,
    diagnostics: Option<&'a crate::diagnostics::DiagnosticSink>,
    progress: Option<&'a dyn NativeProgressSink>,
    pub(crate) output_pipeline: bool,
}

impl<'a> NativeCommandContext<'a> {
    /// Creates the runtime context passed to one native-command execution.
    pub fn new(config: &'a ResolvedConfig, runtime_hints: RuntimeHints) -> Self {
        Self {
            config,
            runtime_hints,
            invocation_elevation: InvocationElevation::Ordinary,
            session_context: NativeSessionContext::default(),
            repl_feedback: Default::default(),
            command_scope: Default::default(),
            diagnostics: None,
            progress: None,
            output_pipeline: false,
        }
    }

    /// Attaches command-local sanitized service evidence.
    pub fn with_diagnostic_sink(mut self, sink: &'a crate::diagnostics::DiagnosticSink) -> Self {
        self.diagnostics = Some(sink);
        self
    }

    /// Returns this command's evidence sink, when hosted.
    pub fn diagnostics(&self) -> Option<&'a crate::diagnostics::DiagnosticSink> {
        self.diagnostics
    }

    /// Sets where this invocation's reader types suggested commands.
    pub fn with_command_scope(mut self, scope: crate::core::CommandScope) -> Self {
        self.command_scope = scope;
        self
    }

    /// Returns the reader's command scope for pure output helpers.
    pub fn command_scope(&self) -> &crate::core::CommandScope {
        &self.command_scope
    }

    /// Renders a root-form command for the reader's current prompt.
    pub fn command(&self, line: &str) -> String {
        self.command_scope.command(line)
    }

    /// Produces a Run hint and retains its root-form command for editor recall.
    /// The editor inserts the command only; Enter still controls execution.
    pub fn run_hint(&self, command: &str, suffix: &str) -> String {
        self.repl_feedback.remember_hint(command);
        format!("hint: Run {}{suffix}", self.command(command))
    }

    /// Whether this invocation transforms its result with output pipe stages.
    pub fn has_output_pipeline(&self) -> bool {
        self.output_pipeline
    }

    /// Whether this invocation may read a confirmation from its terminal.
    /// Requires stdin and stderr terminals, non-JSON output and no output pipeline.
    /// Credential acquisition owns any additional command-scope restrictions.
    pub fn may_prompt(&self) -> bool {
        self.human_output() && std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
    }

    /// Whether live or preliminary documents can be presented on stderr.
    pub fn can_present(&self) -> bool {
        self.human_output() && self.progress.is_some_and(|sink| sink.stderr_is_terminal())
    }

    fn human_output(&self) -> bool {
        !self.output_pipeline
            && self.runtime_hints.format != crate::core::output::OutputFormat::Json
    }

    /// Attaches explicit elevation intent to this execution only.
    pub fn with_invocation_elevation(mut self, elevation: InvocationElevation) -> Self {
        self.invocation_elevation = elevation;
        self
    }

    /// Attaches session-scoped native context to this execution.
    pub fn with_session_context(mut self, session_context: NativeSessionContext) -> Self {
        self.session_context = session_context;
        self
    }

    /// Attaches the host-owned structured progress boundary for this run.
    pub fn with_progress_sink(mut self, progress: &'a dyn NativeProgressSink) -> Self {
        self.progress = Some(progress);
        self
    }

    /// Gives the host an opportunity to display coalesced terminal progress.
    ///
    /// Call before a blocking interaction that must follow pending progress.
    /// May wait for rendering and return a deferred render error. Without a
    /// progress sink, this is a no-op.
    pub fn flush_progress(&self) -> Result<()> {
        if let Some(progress) = self.progress {
            progress.flush()?;
        }
        Ok(())
    }

    /// Requests a rate-limited terminal bell after this REPL invocation flushes.
    ///
    /// The master sound setting, JSON/basic/one-shot suppression and actual
    /// terminal ownership are host policy. This never adds bytes to a payload.
    pub fn request_bell(&self) {
        crate::repl::notices::request_foreground_bell(self.config, self.runtime_hints.format);
    }

    /// Display a notice or confirmation through the host renderer, outside stream filters.
    ///
    /// This is a no-op unless [`Self::can_present`] permits terminal frames.
    pub fn present(&self, document: NativeProgressEvent) -> Result<()> {
        if !self.can_present() {
            return Ok(());
        }
        match self.progress {
            Some(progress) => progress.present(document),
            None => Ok(()),
        }
    }

    /// Draws a preliminary document and reports whether the host can replace it.
    /// Returns false without emitting when [`Self::can_present`] is false.
    pub fn emit_progress_measured(&self, event: NativeProgressEvent) -> Result<bool> {
        if !self.can_present() {
            return Ok(false);
        }
        match self.progress {
            Some(sink) => sink.emit_measured(event),
            None => Ok(false),
        }
    }

    /// Submits one structured transient progress document to the host.
    ///
    /// The host may coalesce terminal replacement updates before displaying them.
    /// Events remain available to stream consumers even without a terminal;
    /// use [`Self::present`] or [`Self::emit_progress_measured`] for frames.
    /// A context created outside the host has no sink, in which case emission
    /// is a no-op. Native commands should still return the stable final
    /// document through [`NativeCommandOutcome`].
    pub fn emit_progress(&self, event: NativeProgressEvent) -> Result<()> {
        match self.progress {
            Some(progress) => progress.emit(event),
            None => Ok(()),
        }
    }
}

/// One transient structured document emitted while a native command runs.
#[derive(Debug, Clone)]
pub struct NativeProgressEvent {
    /// Canonical progress data submitted to the host for rendering.
    pub data: serde_json::Value,
    /// Structured messages attached to this progress document.
    pub messages: Vec<ResponseMessageV1>,
    /// Rendering hints interpreted by the same host pipeline as final output.
    pub meta: ResponseMetaV1,
    /// Historical input: retain it in event streams without animating a terminal replay.
    pub replay: bool,
}

impl NativeProgressEvent {
    /// Creates a progress event with default rendering metadata.
    pub fn new(data: impl Into<serde_json::Value>) -> Self {
        Self {
            data: data.into(),
            messages: Vec::new(),
            meta: ResponseMetaV1::default(),
            replay: false,
        }
    }

    /// Attaches rendering metadata to the progress document.
    pub fn with_meta(mut self, meta: ResponseMetaV1) -> Self {
        self.meta = meta;
        self
    }

    /// Attaches structured messages to the progress document.
    pub fn with_messages(mut self, messages: Vec<ResponseMessageV1>) -> Self {
        self.messages = messages;
        self
    }
}

/// Host boundary that renders transient native-command progress.
pub trait NativeProgressSink {
    /// Whether the presentation destination is a terminal. Hosts with captured
    /// output should report their sink's capability instead of process stderr.
    fn stderr_is_terminal(&self) -> bool {
        std::io::stderr().is_terminal()
    }

    /// Renders or records one progress document before command execution resumes.
    fn emit(&self, event: NativeProgressEvent) -> Result<()>;

    /// Draws immediately and reports whether terminal replacement is available.
    fn emit_measured(&self, event: NativeProgressEvent) -> Result<bool> {
        self.emit(event)?;
        Ok(false)
    }

    /// Render a notice or confirmation without applying result-stream filters.
    fn present(&self, document: NativeProgressEvent) -> Result<()> {
        self.emit(document)
    }

    /// Flushes coalesced terminal progress when the command is waiting for input.
    fn flush(&self) -> Result<()> {
        Ok(())
    }
}

/// Compact prompt view of one visible entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePromptContextEntry {
    /// Short label rendered in compact prompt surfaces.
    pub label: String,
    /// Current value for the context entry.
    pub value: String,
}

/// One operator-visible value remembered for the current REPL session.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeSessionContextEntry {
    /// Owning integration.
    pub service: String,
    /// Profile at selection.
    pub profile: String,
    /// Effective identity at selection.
    pub identity: String,
    /// Service endpoint binding.
    pub origin: String,
    /// Last command that consumed this entry.
    pub used_by: String,
    /// Subject kind from the shared entity vocabulary.
    pub kind: String,
    /// Latest value stored for this kind.
    pub value: String,
    /// Command path that last set this value.
    pub command: String,
    /// UTC timestamp in RFC 3339 format when the value was last set.
    pub when: String,
}

impl NativeSessionContextEntry {
    pub(crate) fn key(&self) -> ContextKey {
        if crate::context::shared(&self.kind) {
            (
                self.kind.clone(),
                String::new(),
                self.profile.clone(),
                self.identity.clone(),
                String::new(),
            )
        } else {
            (
                self.kind.clone(),
                self.service.clone(),
                self.profile.clone(),
                self.identity.clone(),
                self.origin.clone(),
            )
        }
    }
}

/// Trusted REPL commands for moving between pages produced by a native
/// command.
///
/// The host only accepts this context from native commands. Plugin responses
/// cannot publish commands here, so a provider cannot make the REPL execute
/// an arbitrary continuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePagination {
    /// Tokenized command that produced the current page. The REPL replays it
    /// to step back here, so commands never compute a previous page.
    pub current: Vec<String>,
    /// Tokenized command to run for the next page, if one exists.
    pub next: Option<Vec<String>>,
}

#[derive(Default)]
struct PaginationState {
    page: Option<NativePagination>,
    /// Commands for the pages already stepped past, newest last.
    back: Vec<Vec<String>>,
    /// Back stack handed to the page the REPL is about to run; it applies only
    /// if that command publishes pagination again.
    carried: Option<Vec<Vec<String>>>,
}

type ContextKey = (String, String, String, String, String);

/// Session-scoped context shared between native commands, plugins, and the REPL host.
///
/// Private workflow caches stay in memory. Declared entity selections are
/// visible in the prompt and `context`, scoped to service, identity and profile.
/// Services may opt specific kinds into REPL-only persistence.
#[derive(Clone, Default)]
pub struct NativeSessionContext {
    pub(crate) values: Arc<RwLock<BTreeMap<String, String>>>,
    pub(crate) context_entries: Arc<RwLock<BTreeMap<ContextKey, NativeSessionContextEntry>>>,
    pub(crate) binding: Arc<RwLock<crate::context::ContextBinding>>,
    pub(crate) service: String,
    pagination: Arc<RwLock<PaginationState>>,
    completion_refresh_requested: Arc<AtomicBool>,
    pub(crate) command_path: Option<Arc<str>>,
}

impl fmt::Debug for NativeSessionContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeSessionContext")
            .field("values", &"[redacted]")
            .field("context_entries", &"[redacted]")
            .field("pagination", &"[redacted]")
            .field(
                "completion_refresh_requested",
                &self.completion_refresh_requested,
            )
            .finish()
    }
}

impl NativeSessionContext {
    /// Stores one in-memory value for later native commands in this session.
    pub fn set_value(&self, key: impl Into<String>, value: impl Into<String>) {
        if let Ok(mut values) = self.values.write() {
            values.insert(key.into(), value.into());
        }
    }

    /// Returns one previously stored in-memory session value.
    pub fn value(&self, key: &str) -> Option<String> {
        self.values
            .read()
            .ok()
            .and_then(|values| values.get(key).cloned())
    }

    /// Stores one operator-visible value for this REPL session.
    ///
    /// The active native command path is recorded as the source. Plugin
    /// responses use the same store after the host receives their context
    /// updates.
    pub fn set_context_value(&self, kind: impl Into<String>, value: impl Into<String>) {
        let kind = context_kind(&kind.into());
        let value = value.into();
        if value.trim().is_empty() {
            self.clear_context(Some(&kind));
            return;
        }
        if !crate::context::valid_value(&value) {
            return;
        }
        let command = self
            .command_path
            .as_deref()
            .unwrap_or("context set")
            .to_string();
        let binding = self.binding.read().unwrap_or_else(|e| e.into_inner());
        let profile = binding.profile.clone();
        let identity = binding.identity.clone();
        let origin = binding
            .origins
            .get(&self.service)
            .cloned()
            .unwrap_or_default();
        let service = self.service.clone();
        let when = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        if let Ok(mut entries) = self.context_entries.write() {
            let entry = NativeSessionContextEntry {
                kind: kind.clone(),
                value,
                command,
                when,
                service,
                profile,
                identity,
                origin,
                used_by: String::new(),
            };
            entries.insert(entry.key(), entry);
        }
    }

    /// Restores an entry with its original scope and provenance during migration.
    pub fn restore_context_entry(&self, entry: NativeSessionContextEntry) {
        if !crate::context::valid_value(&entry.value) {
            return;
        }
        if let Ok(mut entries) = self.context_entries.write() {
            entries.entry(entry.key()).or_insert(entry);
        }
    }

    /// Returns context visible to the current profile and identity.
    pub fn context_entries(&self) -> Vec<NativeSessionContextEntry> {
        let binding = self.binding.read().unwrap_or_else(|e| e.into_inner());
        self.context_entries
            .read()
            .map(|entries| {
                entries
                    .values()
                    .filter(|entry| {
                        entry.profile == binding.profile
                            && entry.identity == binding.identity
                            && (entry.service.is_empty()
                                || binding
                                    .origins
                                    .get(&entry.service)
                                    .is_none_or(|origin| origin == &entry.origin))
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Clears visible selections in this service, or all services when unbound.
    /// The host saves persisted kinds after the command.
    pub fn clear_context(&self, kind: Option<&str>) {
        let visible: Vec<_> = self
            .context_entries()
            .into_iter()
            .filter(|entry| {
                self.service.is_empty()
                    || crate::context::shared(&entry.kind)
                    || entry.service == self.service
            })
            .collect();
        if let Ok(mut entries) = self.context_entries.write() {
            entries.retain(|_, entry| {
                !visible.contains(entry) || kind.is_some_and(|k| k != entry.kind)
            });
        }
    }

    /// Returns a visible entity in this service's scope.
    pub fn context_value(&self, kind: &str) -> Option<String> {
        self.context_entries()
            .into_iter()
            .find(|entry| {
                entry.kind == kind
                    && (crate::context::shared(kind) || entry.service == self.service)
            })
            .map(|entry| entry.value)
    }

    /// Binds a command to its owning service without copying its selections.
    pub fn for_service(&self, service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            ..self.clone()
        }
    }

    /// Clears entries belonging to a logged-out service.
    pub fn clear_service_context(&self) {
        if let Ok(mut entries) = self.context_entries.write() {
            entries.retain(|_, entry| entry.service != self.service);
        }
    }

    pub(crate) fn for_command(&self, command_path: impl Into<String>) -> Self {
        Self {
            command_path: Some(Arc::from(command_path.into())),
            ..self.clone()
        }
    }

    /// Returns prompt entries using the same labels as the tutorial renderer.
    pub fn prompt_entries(&self) -> Vec<NativePromptContextEntry> {
        let entries = self.context_entries();
        entries
            .iter()
            .map(|entry| NativePromptContextEntry {
                label: if entries
                    .iter()
                    .filter(|other| other.kind == entry.kind)
                    .count()
                    > 1
                {
                    format!("{}/{}", entry.service, entry.kind)
                } else {
                    entry.kind.clone()
                },
                value: entry.value.clone(),
            })
            .collect()
    }

    /// Renders one compact prompt token, also suitable for instructional text.
    pub fn render_context_token(kind: &str, value: &str) -> String {
        format!("{kind}:{value}")
    }

    pub(crate) fn for_editor_action(&self) -> Self {
        Self {
            pagination: Arc::new(RwLock::new(PaginationState::default())),
            ..self.clone()
        }
    }

    /// Replaces the trusted native-command pagination context for this REPL.
    pub fn set_pagination(&self, pagination: NativePagination) {
        if let Ok(mut state) = self.pagination.write() {
            state.back = state.carried.take().unwrap_or_default();
            state.page = Some(pagination);
        }
    }

    /// Returns the current native-command pagination context, if any.
    pub fn pagination(&self) -> Option<NativePagination> {
        self.pagination
            .read()
            .ok()
            .and_then(|state| state.page.clone())
    }

    /// Clears any native-command pagination context.
    pub fn clear_pagination(&self) {
        if let Ok(mut state) = self.pagination.write() {
            state.page = None;
            state.back.clear();
        }
    }

    /// Whether the REPL can step back to an earlier page.
    pub(crate) fn has_previous_page(&self) -> bool {
        self.pagination
            .read()
            .is_ok_and(|state| state.page.is_some() && !state.back.is_empty())
    }

    /// Picks the command for the next or previous page and carries the back
    /// stack it implies over to that page. `None` at either boundary.
    pub(crate) fn step_page(&self, forward: bool) -> Option<Vec<String>> {
        let mut state = self.pagination.write().ok()?;
        let page = state.page.as_ref()?;
        let mut back = state.back.clone();
        let command = if forward {
            let next = page.next.clone()?;
            back.push(page.current.clone());
            next
        } else {
            back.pop()?
        };
        state.carried = Some(back);
        Some(command)
    }

    /// Drops a carried back stack the stepped-to command did not claim.
    pub(crate) fn finish_page_step(&self) {
        if let Ok(mut state) = self.pagination.write() {
            state.carried = None;
        }
    }

    /// Requests rebuilding the active REPL completion tree after execution.
    ///
    /// Native commands should call this after replacing completion data held in
    /// their own shared snapshot. Outside an interactive REPL the request has
    /// no observable effect.
    pub fn request_completion_refresh(&self) {
        self.completion_refresh_requested
            .store(true, Ordering::Release);
    }

    pub(crate) fn take_completion_refresh_request(&self) -> bool {
        self.completion_refresh_requested
            .swap(false, Ordering::AcqRel)
    }
}

/// Context kinds compare like command names: trimmed and lowercase.
fn context_kind(kind: &str) -> String {
    kind.trim().to_lowercase()
}

/// Result of executing a native command.
pub enum NativeCommandOutcome {
    /// Return rendered help text directly.
    Help(String),
    /// Return a protocol response payload.
    Response(Box<ResponseV1>),
    /// Return structured output and then terminate with an explicit status.
    ResponseWithExit {
        /// Final response that still flows through JSON/DSL rendering.
        response: Box<ResponseV1>,
        /// Process status returned after rendering the response.
        exit_code: i32,
    },
    /// Exit immediately with the given status code.
    Exit(i32),
}

impl NativeCommandOutcome {
    /// Treats help, version and missing-subcommand requests as help documents;
    /// preserves all other clap failures as errors.
    pub fn from_clap(error: clap::Error) -> Result<Self> {
        if is_clap_help_error(error.kind()) {
            Ok(Self::Help(error.to_string()))
        } else {
            Err(error.into())
        }
    }
}

pub(crate) fn is_clap_help_error(kind: clap::error::ErrorKind) -> bool {
    matches!(
        kind,
        clap::error::ErrorKind::DisplayHelp
            | clap::error::ErrorKind::DisplayVersion
            | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            | clap::error::ErrorKind::MissingSubcommand
    )
}

/// Trait implemented by in-process commands registered alongside plugins.
pub trait NativeCommand: Send + Sync {
    /// Applies the resolved host configuration before help, policy, completion,
    /// or execution reads this command's state. Called again on REPL rebuild.
    /// Cloned registries share command instances, so embedders must serialize
    /// invocations that use `configure` to select mutable session state.
    ///
    /// Keep this local: select the service/session context and invalidate stale
    /// snapshots here; remote refresh belongs in execution, invocation metadata
    /// preparation, or refresh_completion.
    /// The registered command name must remain stable across configurations.
    fn configure(&self, _config: &ResolvedConfig) {}

    /// Returns the unbuilt clap command definition for this command.
    ///
    /// The host sets its invocation-specific binary name before clap caches usage
    /// names. Build a separate copy when projecting implicit positional indices
    /// into custom descriptions or completion metadata.
    fn command(&self) -> Command;

    /// Converts supplied arguments to the canonical command grammar.
    ///
    /// The host calls this before native clap parsing and command-path
    /// authorization, then passes the canonical arguments to [`Self::execute`].
    /// If [`Self::prepare_invocation_metadata`] changes the grammar, the host
    /// normalizes the original arguments again before retrying strict parsing.
    /// Arguments exclude the registered command name and any DSL pipeline.
    /// The default leaves them unchanged.
    ///
    /// Implementations may expand command shorthand or map advertised dynamic
    /// flags. Preserve explicit help/version requests and user-supplied values;
    /// do not add inferred request constraints. This runs before authorization:
    /// use only local configuration or cached metadata, without network calls
    /// or execution side effects. Return an error for ambiguous normalization.
    fn normalize_args(&self, args: &[String]) -> Result<Vec<String>> {
        Ok(args.to_vec())
    }

    /// Prepares metadata when an invocation fails strict parsing or requests help.
    ///
    /// For ordinary execution, the host checks cached-path visibility and runs
    /// the product access hook before calling this. For help, it checks cached
    /// visibility and treats preparation errors as a cache miss so help
    /// remains available offline and without acquiring credentials. Arguments
    /// are the original supplied arguments, without the registered command name
    /// or DSL pipeline. Remote discovery is permitted, but this must not execute
    /// the requested action. Use the supplied command scope for recovery hints.
    /// Return `true` only when the grammar changed: the host then normalizes,
    /// describes, and strictly parses the invocation once more, and authorizes
    /// its full path against refreshed native declarations and product visibility
    /// before execution. The product hook is not called again after refresh.
    fn prepare_invocation_metadata(
        &self,
        _args: &[String],
        _config: &ResolvedConfig,
        _command_scope: &crate::core::CommandScope,
    ) -> Result<bool> {
        Ok(false)
    }

    /// Returns optional auth/visibility metadata for the command.
    fn auth(&self) -> Option<DescribeCommandAuthV1> {
        None
    }

    /// Returns whether this native command accepts explicit invocation
    /// elevation metadata.
    ///
    /// Product wrappers may opt in when they own the elevated credential and
    /// server-side authorization flow. The host never acquires or persists
    /// that credential.
    fn supports_invocation_elevation(&self) -> bool {
        false
    }

    /// Endpoint binding used to hide selections after a service switch.
    fn context_origin(&self, _config: &ResolvedConfig) -> String {
        String::new()
    }

    /// Service-local kinds available before dynamic command metadata is loaded.
    fn context_kinds(&self) -> &'static [&'static str] {
        &[]
    }

    /// Imports an older service-owned selection file into the shared store.
    fn migrate_context(&self, _session: &NativeSessionContext) -> Result<()> {
        Ok(())
    }

    /// Kinds persisted by this service, loaded before the first REPL prompt.
    fn persisted_context_kinds(&self) -> &'static [&'static str] {
        &[]
    }

    /// Builds the plugin-protocol style description for this command.
    fn describe(&self) -> DescribeCommandV1 {
        let mut describe = DescribeCommandV1::from_clap(self.command());
        describe.auth = self.auth();
        describe.inherit_auth_from_parents();
        describe
    }

    /// Projects the command grammar and cached metadata into completion nodes.
    ///
    /// Override this when cached suggestions are also needed in noninteractive
    /// catalogs. Interactive-only enrichment belongs in `augment_completion`.
    fn completion(&self) -> CompletionNode {
        CompletionNode::from_clap(&self.command())
    }

    /// Adds runtime-owned suggestions to the otherwise static completion tree.
    ///
    /// The host calls this only while preparing an interactive completion
    /// surface. Implementations should cache any remote data they use so REPL
    /// editing never performs network I/O per keystroke.
    fn augment_completion(&self, _completion: &mut CompletionNode) {}

    /// Returns a cached, single-line product status for the right prompt.
    ///
    /// Called during editor paints: never perform I/O or wait for a worker here.
    /// The host sanitizes text and omits it in basic and JSON sessions.
    fn repl_status(&self) -> Option<crate::repl::ReplStatus> {
        None
    }

    /// Named editor actions offered by this product, resolved against config.
    fn repl_actions(&self, _config: &ResolvedConfig) -> Result<Vec<crate::repl::ReplAction>> {
        Ok(Vec::new())
    }

    /// Runs a registered editor action with ordinary invocation context.
    ///
    /// The host suspends editor output and preserves draft, cursor, history,
    /// pagination and shell scope. This does not run the interactive access hook:
    /// missing credentials must return guidance without an implicit login.
    fn execute_repl_action(
        &self,
        _id: &str,
        _context: &NativeCommandContext<'_>,
    ) -> Result<NativeCommandOutcome> {
        anyhow::bail!("unknown editor action")
    }

    /// Reports actual REPL input selection without blocking startup.
    ///
    /// Rich input supplies a cancellable lifetime. Basic input supplies `None`
    /// for existing startup behavior only: do not start recurring work or audio.
    /// Session-wide JSON never calls this hook. Stop rich work on cancellation.
    fn start_repl(&self, _config: &ResolvedConfig, _session: Option<crate::repl::ReplSession>) {}

    /// Refreshes runtime completion data on a host-owned background thread.
    ///
    /// The host invokes this once when an authorized interactive REPL starts.
    /// Implementations may perform remote I/O here, then atomically replace
    /// the snapshot read by [`NativeCommand::augment_completion`].
    fn refresh_completion(&self) -> Result<()> {
        Ok(())
    }

    /// Executes the command using already-parsed argument tokens.
    ///
    /// `args` contains the tokens after the registered command name. For a
    /// command registered as `history`, the command line `osp history clear
    /// --all` reaches `execute` as `["clear", "--all"]`.
    ///
    /// The host interprets outcomes as follows:
    ///
    /// - [`NativeCommandOutcome::Help`] is rendered as a help/guide response
    /// - [`NativeCommandOutcome::Exit`] terminates the command immediately with
    ///   that exit code
    /// - [`NativeCommandOutcome::Response`] is treated like plugin protocol
    ///   output and may still flow through trailing DSL stages
    /// - [`NativeCommandOutcome::ResponseWithExit`] follows the same output
    ///   path, then returns the supplied process status
    ///
    /// Long-running commands may call [`NativeCommandContext::emit_progress`].
    /// The host renders each transient document immediately on stderr so the
    /// final stdout document remains safe for JSON and DSL consumers.
    ///
    /// Return `Err` when command execution itself fails. The host formats that
    /// failure like other command errors.
    ///
    /// # Examples
    ///
    /// ```
    /// use anyhow::Result;
    /// use clap::Command;
    /// use osp_cli::{NativeCommand, NativeCommandContext, NativeCommandOutcome};
    ///
    /// struct HistoryCommand;
    ///
    /// impl NativeCommand for HistoryCommand {
    ///     fn command(&self) -> Command {
    ///         Command::new("history").about("Manage local history")
    ///     }
    ///
    ///     fn execute(
    ///         &self,
    ///         args: &[String],
    ///         _context: &NativeCommandContext<'_>,
    ///     ) -> Result<NativeCommandOutcome> {
    ///         match args {
    ///             [subcommand, flag] if subcommand == "clear" && flag == "--all" => {
    ///                 Ok(NativeCommandOutcome::Exit(0))
    ///             }
    ///             _ => Ok(NativeCommandOutcome::Help(
    ///                 "usage: history clear --all".to_string(),
    ///             )),
    ///         }
    ///     }
    /// }
    /// ```
    fn execute(
        &self,
        args: &[String],
        context: &NativeCommandContext<'_>,
    ) -> Result<NativeCommandOutcome>;
}

/// Registry of in-process native commands exposed alongside plugin commands.
#[derive(Clone, Default)]
#[must_use]
pub struct NativeCommandRegistry {
    commands: Arc<BTreeMap<String, Arc<dyn NativeCommand>>>,
    completion_refresh_requested: Arc<AtomicBool>,
}

impl NativeCommandRegistry {
    pub(crate) fn configure(&self, config: &ResolvedConfig) {
        for command in self.commands.values() {
            command.configure(config);
        }
    }

    /// Creates an empty native command registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a registry with one additional registered command.
    ///
    /// # Examples
    ///
    /// ```text
    /// use anyhow::Result;
    /// use clap::Command;
    /// use osp_cli::{
    ///     NativeCommand, NativeCommandContext, NativeCommandOutcome, NativeCommandRegistry,
    /// };
    ///
    /// struct VersionCommand;
    ///
    /// impl NativeCommand for VersionCommand {
    ///     fn command(&self) -> Command {
    ///         Command::new("version").about("Show version")
    ///     }
    ///
    ///     fn execute(
    ///         &self,
    ///         _args: &[String],
    ///         _context: &NativeCommandContext<'_>,
    ///     ) -> Result<NativeCommandOutcome> {
    ///         Ok(NativeCommandOutcome::Exit(0))
    ///     }
    /// }
    ///
    /// let registry = NativeCommandRegistry::new().with_command(VersionCommand);
    /// let catalog = registry.catalog();
    ///
    /// assert!(registry.command(" VERSION ").is_some());
    /// assert_eq!(catalog[0].name, "version");
    /// assert_eq!(catalog[0].about, "Show version");
    /// assert!(catalog[0].auth.is_none());
    /// ```
    pub fn with_command(mut self, command: impl NativeCommand + 'static) -> Self {
        self.register(command);
        self
    }

    /// Registers or replaces a native command by normalized command name.
    pub fn register(&mut self, command: impl NativeCommand + 'static) {
        let mut next = (*self.commands).clone();
        let command = Arc::new(command) as Arc<dyn NativeCommand>;
        let name = normalize_name(command.command().get_name());
        next.insert(name, command);
        self.commands = Arc::new(next);
    }

    /// Returns `true` when no native commands are registered.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Returns a registered command by normalized name.
    ///
    /// Lookup is case- and surrounding-whitespace-insensitive so callers can
    /// reuse human-typed names without normalizing them first.
    pub fn command(&self, name: &str) -> Option<&Arc<dyn NativeCommand>> {
        self.commands.get(&normalize_name(name))
    }

    /// Returns catalog metadata for all registered native commands.
    pub(crate) fn catalog(&self) -> Vec<CommandCatalogEntry> {
        self.catalog_with_completion(false)
    }

    /// Returns catalog metadata with native runtime completion additions.
    pub(crate) fn completion_catalog(&self) -> Vec<CommandCatalogEntry> {
        self.catalog_with_completion(true)
    }

    pub(crate) fn take_completion_refresh_request(&self) -> bool {
        self.completion_refresh_requested
            .swap(false, Ordering::AcqRel)
    }

    pub(crate) fn refresh_completion_in_background(&self, should_refresh: impl Fn(&str) -> bool) {
        for (name, command) in self.commands.iter() {
            if !should_refresh(name) {
                continue;
            }
            let name = name.clone();
            let command = Arc::clone(command);
            let completion_refresh_requested = Arc::clone(&self.completion_refresh_requested);
            if let Err(error) = std::thread::Builder::new()
                .name(format!("osp-completion-{name}"))
                .spawn(move || {
                    if let Err(error) = command.refresh_completion() {
                        tracing::debug!(command = %name, %error, "native completion refresh failed");
                    }
                    completion_refresh_requested.store(true, Ordering::Release);
                })
            {
                tracing::warn!(%error, "failed to start native completion refresh");
            }
        }
    }

    fn catalog_with_completion(&self, augment_completion: bool) -> Vec<CommandCatalogEntry> {
        self.commands
            .values()
            .map(|command| {
                let mut describe = command.describe();
                describe.inherit_auth_from_parents();
                let mut completion = command.completion();
                if augment_completion {
                    command.augment_completion(&mut completion);
                }
                CommandCatalogEntry {
                    name: completion.name.clone(),
                    about: crate::core::clap_metadata::plain(command.command().get_about())
                        .unwrap_or_default(),
                    auth: describe.auth.clone(),
                    subcommands: command
                        .command()
                        .get_subcommands()
                        .filter(|child| !child.is_hide_set())
                        .map(|child| child.get_name().to_string())
                        .collect(),
                    completion,
                    source: CommandSource::Native,
                }
            })
            .collect()
    }

    /// Collects inherited wire auth by canonical command path.
    pub(crate) fn command_auth(&self) -> BTreeMap<CommandPath, DescribeCommandAuthV1> {
        let mut registry = BTreeMap::new();
        for command in self.commands.values() {
            let mut describe = command.describe();
            describe.inherit_auth_from_parents();
            collect_command_auth(&mut registry, &describe, &[]);
        }
        registry
    }
}

fn normalize_name(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests;
