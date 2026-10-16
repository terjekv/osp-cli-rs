//! Opt-in startup timing recorded from the host's tracing spans.

use std::fmt::Write as FmtWrite;
use std::io::{self, Write};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id};
use tracing::{Span, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{Layer, filter::filter_fn};

static TRACE_STATE: OnceLock<Mutex<TraceState>> = OnceLock::new();
static FIRST_PROMPT: Mutex<Option<Span>> = Mutex::new(None);

#[derive(Clone, Copy)]
enum TraceFormat {
    Human,
    Json,
}

struct TraceState {
    format: Option<TraceFormat>,
    started: Instant,
    phases: Vec<TracePhase>,
    finished: bool,
}

#[derive(Serialize)]
struct TraceDocument<'a> {
    total_ms: f64,
    phases: &'a [TracePhase],
}

#[derive(Serialize)]
struct TracePhase {
    name: String,
    start_offset_ms: f64,
    duration_ms: f64,
    depth: usize,
    parent: Option<usize>,
    share_percent: f64,
    #[serde(skip)]
    started: Instant,
    #[serde(skip)]
    ended: bool,
}

/// Installs the subscriber before the first startup span, without resetting
/// logging settings when the host or editor is initialized again.
pub(crate) fn initialize() {
    let _ = trace_state();
    crate::app::logging::initialize_developer_logging();
}

fn trace_state() -> &'static Mutex<TraceState> {
    TRACE_STATE.get_or_init(|| {
        let started = Instant::now();
        let format = match std::env::var("OSP_TRACE_STARTUP").ok() {
            Some(value) if value == "1" => Some(TraceFormat::Human),
            Some(value) if value.eq_ignore_ascii_case("json") => Some(TraceFormat::Json),
            _ => None,
        };
        Mutex::new(TraceState {
            format,
            started,
            phases: Vec::new(),
            finished: false,
        })
    })
}

fn enabled() -> bool {
    let state = trace_state().lock().unwrap_or_else(PoisonError::into_inner);
    state.format.is_some() && !state.finished
}

/// Collects only startup spans; the normal logging layers retain their own filters.
pub(crate) fn layer<S>() -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    StartupTraceLayer.with_filter(filter_fn(|metadata| {
        metadata.target() == "osp_startup" && enabled()
    }))
}

struct StartupTraceLayer;
struct PhaseIndex(usize);

impl<S> Layer<S> for StartupTraceLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let parent = span
            .scope()
            .skip(1)
            .find_map(|parent| parent.extensions().get::<PhaseIndex>().map(|index| index.0));
        let mut name = PhaseName(attrs.metadata().name().to_string());
        attrs.record(&mut name);
        let mut state = trace_state().lock().unwrap_or_else(PoisonError::into_inner);
        if state.finished {
            return;
        }
        let started = Instant::now();
        let index = state.phases.len();
        let depth = parent.map_or(0, |index| state.phases[index].depth + 1);
        let start_offset_ms = started.duration_since(state.started).as_secs_f64() * 1_000.0;
        state.phases.push(TracePhase {
            name: name.0,
            start_offset_ms,
            duration_ms: 0.0,
            depth,
            parent,
            share_percent: 0.0,
            started,
            ended: false,
        });
        span.extensions_mut().insert(PhaseIndex(index));
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };
        let extensions = span.extensions();
        let Some(index) = extensions.get::<PhaseIndex>() else {
            return;
        };
        let mut state = trace_state().lock().unwrap_or_else(PoisonError::into_inner);
        if !state.finished {
            let phase = &mut state.phases[index.0];
            phase.duration_ms = phase.started.elapsed().as_secs_f64() * 1_000.0;
            phase.ended = true;
        }
    }
}

struct PhaseName(String);

impl Visit for PhaseName {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "name" {
            self.0 = format!("{value:?}");
        }
    }
}

/// Prints the trace once. Open phases end here, including a one-shot command
/// that returns before all of its span handles have been dropped.
pub(crate) fn finish() {
    let output = {
        let mut state = trace_state().lock().unwrap_or_else(PoisonError::into_inner);
        let Some(format) = state.format else { return };
        if state.finished {
            return;
        }
        state.finished = true;
        let finished_at = Instant::now();
        let total_ms = finished_at.duration_since(state.started).as_secs_f64() * 1_000.0;
        for phase in &mut state.phases {
            if !phase.ended {
                phase.duration_ms =
                    finished_at.duration_since(phase.started).as_secs_f64() * 1_000.0;
            }
            phase.share_percent = if total_ms > 0.0 {
                phase.duration_ms / total_ms * 100.0
            } else {
                0.0
            };
        }
        let document = TraceDocument {
            total_ms,
            phases: &state.phases,
        };
        match format {
            TraceFormat::Json => serde_json::to_string(&document).ok().map(|mut output| {
                output.push('\n');
                output
            }),
            TraceFormat::Human => Some(render_human_trace(&document)),
        }
    };
    if let Some(output) = output {
        let stderr = io::stderr();
        let mut stderr = stderr.lock();
        let _ = stderr.write_all(output.as_bytes());
        let _ = stderr.flush();
    }
}

fn render_human_trace(document: &TraceDocument<'_>) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "Startup trace total={:.3}ms", document.total_ms);
    for phase in document.phases {
        let indentation = "  ".repeat(phase.depth);
        let _ = writeln!(
            output,
            "{indentation}{:9.3}ms +{:8.3}ms {:6.2}% {}",
            phase.start_offset_ms, phase.duration_ms, phase.share_percent, phase.name
        );
    }
    output
}

/// Holds the paint span across editor callbacks until the first prompt appears.
pub(crate) fn begin_first_prompt() {
    if !enabled() {
        return;
    }
    let mut first_prompt = FIRST_PROMPT.lock().unwrap_or_else(PoisonError::into_inner);
    if first_prompt.is_none() {
        *first_prompt = Some(tracing::info_span!(target: "osp_startup", "first prompt paint"));
    }
}

/// Closes the first-prompt span and prints the trace. Later prompts are no-ops.
pub(crate) fn first_prompt_rendered() {
    let phase = FIRST_PROMPT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if phase.is_some() {
        drop(phase);
        finish();
    }
}
