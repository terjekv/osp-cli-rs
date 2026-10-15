//! Opt-in process startup timing, independent from developer logging.

use std::cell::RefCell;
use std::fmt::Write as FmtWrite;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

use serde::Serialize;

const TRACE_ENV: &str = "OSP_TRACE_STARTUP";

static TRACE_STATE: OnceLock<Mutex<TraceState>> = OnceLock::new();
static TRACE_ENABLED: AtomicBool = AtomicBool::new(false);
static FIRST_PROMPT: Mutex<Option<PhaseGuard>> = Mutex::new(None);

thread_local! {
    static PHASE_STACK: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy)]
enum TraceFormat {
    Human,
    Json,
}

struct TraceState {
    format: Option<TraceFormat>,
    started: Instant,
    phases: Vec<PhaseRecord>,
    finished: bool,
}

struct PhaseRecord {
    name: String,
    started: Instant,
    ended: Option<Instant>,
    parent: Option<usize>,
}

#[derive(Serialize)]
struct TraceDocument {
    total_ms: f64,
    phases: Vec<TracePhase>,
}

#[derive(Serialize)]
struct TracePhase {
    name: String,
    start_offset_ms: f64,
    duration_ms: f64,
    depth: usize,
    parent: Option<usize>,
    share_percent: f64,
}

/// Captures the environment setting once, before startup work begins.
pub(crate) fn initialize() {
    let _ = trace_state();
}

/// Starts a named phase only when startup tracing is enabled.
pub(crate) fn phase(name: &'static str) -> PhaseGuard {
    if !TRACE_ENABLED.load(Ordering::Relaxed) {
        return PhaseGuard::disabled();
    }
    begin_phase(name.to_string())
}

/// Starts a dynamically named phase without formatting its name when disabled.
pub(crate) fn phase_with_name(name: impl FnOnce() -> String) -> PhaseGuard {
    if !TRACE_ENABLED.load(Ordering::Relaxed) {
        return PhaseGuard::disabled();
    }
    begin_phase(name())
}

/// Finishes and prints the process trace once. REPL startup finishes it from
/// the first prompt-render callback; one-shot wrappers call it as they return.
pub(crate) fn finish() {
    if let Some(output) = take_output() {
        write_output(&output);
    }
}

fn take_output() -> Option<String> {
    if !TRACE_ENABLED.load(Ordering::Relaxed) {
        return None;
    }

    let state = TRACE_STATE.get()?;
    let (format, document) = {
        let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.finished {
            return None;
        }
        state.finished = true;
        TRACE_ENABLED.store(false, Ordering::Relaxed);
        let finished_at = Instant::now();
        let total = finished_at.saturating_duration_since(state.started);
        let total_seconds = total.as_secs_f64();
        let phases = state
            .phases
            .iter()
            .enumerate()
            .map(|(index, phase)| {
                let phase_end = phase.ended.unwrap_or(finished_at);
                let duration = phase_end.saturating_duration_since(phase.started);
                let depth = phase_depth(&state.phases, index);
                TracePhase {
                    name: phase.name.clone(),
                    start_offset_ms: phase
                        .started
                        .saturating_duration_since(state.started)
                        .as_secs_f64()
                        * 1_000.0,
                    duration_ms: duration.as_secs_f64() * 1_000.0,
                    depth,
                    parent: phase.parent,
                    share_percent: if total_seconds > 0.0 {
                        duration.as_secs_f64() / total_seconds * 100.0
                    } else {
                        0.0
                    },
                }
            })
            .collect();
        (
            state.format,
            TraceDocument {
                total_ms: total.as_secs_f64() * 1_000.0,
                phases,
            },
        )
    };

    match format {
        Some(TraceFormat::Json) => serde_json::to_string(&document).ok().map(|mut output| {
            output.push('\n');
            output
        }),
        Some(TraceFormat::Human) => Some(render_human_trace(&document)),
        None => None,
    }
}

fn trace_state() -> &'static Mutex<TraceState> {
    TRACE_STATE.get_or_init(|| {
        let started = Instant::now();
        let format = match std::env::var_os(TRACE_ENV).and_then(|value| value.into_string().ok()) {
            Some(value) if value == "1" => Some(TraceFormat::Human),
            Some(value) if value.eq_ignore_ascii_case("json") => Some(TraceFormat::Json),
            _ => None,
        };
        TRACE_ENABLED.store(format.is_some(), Ordering::Relaxed);
        Mutex::new(TraceState {
            format,
            started,
            phases: Vec::new(),
            finished: false,
        })
    })
}

fn begin_phase(name: String) -> PhaseGuard {
    let Some(state) = TRACE_STATE.get() else {
        return PhaseGuard::disabled();
    };
    let parent = PHASE_STACK.with(|stack| stack.borrow().last().copied());
    let index = {
        let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.finished {
            return PhaseGuard::disabled();
        }
        let index = state.phases.len();
        state.phases.push(PhaseRecord {
            name,
            started: Instant::now(),
            ended: None,
            parent,
        });
        index
    };
    PHASE_STACK.with(|stack| stack.borrow_mut().push(index));
    PhaseGuard { index: Some(index) }
}

fn end_phase(index: usize) {
    if let Some(state) = TRACE_STATE.get() {
        let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(phase) = state.phases.get_mut(index)
            && phase.ended.is_none()
        {
            phase.ended = Some(Instant::now());
        }
    }
    PHASE_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        if stack.last() == Some(&index) {
            stack.pop();
        } else if let Some(position) = stack.iter().rposition(|active| *active == index) {
            stack.remove(position);
        }
    });
}

fn phase_depth(phases: &[PhaseRecord], index: usize) -> usize {
    let mut depth = 0;
    let mut parent = phases.get(index).and_then(|phase| phase.parent);
    while let Some(parent_index) = parent {
        depth += 1;
        parent = phases.get(parent_index).and_then(|phase| phase.parent);
    }
    depth
}

fn write_output(output: &str) {
    let stderr = io::stderr();
    let mut stderr = stderr.lock();
    let _ = stderr.write_all(output.as_bytes());
    let _ = stderr.flush();
}

fn render_human_trace(document: &TraceDocument) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "Startup trace total={:.3}ms", document.total_ms);
    for phase in &document.phases {
        let indentation = "  ".repeat(phase.depth);
        let _ = writeln!(
            output,
            "{indentation}{:9.3}ms +{:8.3}ms {:6.2}% {}",
            phase.start_offset_ms, phase.duration_ms, phase.share_percent, phase.name
        );
    }
    output
}

pub(crate) struct PhaseGuard {
    index: Option<usize>,
}

impl PhaseGuard {
    fn disabled() -> Self {
        Self { index: None }
    }

    fn finish(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        if let Some(index) = self.index.take() {
            end_phase(index);
        }
    }
}

impl Drop for PhaseGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Opens the phase that ends when the first REPL prompt is rendered.
pub(crate) fn begin_first_prompt() {
    if !TRACE_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let mut first_prompt = FIRST_PROMPT.lock().unwrap_or_else(PoisonError::into_inner);
    if first_prompt.is_none() {
        *first_prompt = Some(phase("first prompt paint"));
    }
}

/// Closes the first-prompt phase and prints the trace. Later prompts are no-ops.
pub(crate) fn first_prompt_rendered() {
    if !TRACE_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let phase = FIRST_PROMPT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(phase) = phase {
        phase.finish();
        finish();
    }
}
