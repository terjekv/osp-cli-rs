//! Developer logging bootstrap and reload support.
//!
//! This module exists to derive logging behavior from startup-time CLI/config
//! inputs and apply it to the process-global tracing subscriber.
//!
//! Contract:
//!
//! - log-level parsing and subscriber wiring live here
//! - callers should provide desired logging settings rather than configuring
//!   tracing ad hoc throughout the app
//!
//! Public API shape:
//!
//! - small logging configs use concrete constructors/factories rather than an
//!   abstract builder layer
//! - [`DeveloperLoggingConfig::new`] creates the exact runtime baseline and
//!   `with_*` setters refine optional outputs

use std::cell::Cell;
#[cfg(test)]
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::{self, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry;
use tracing_subscriber::reload;
use tracing_subscriber::util::SubscriberInitExt;

thread_local! {
    // A command override must not alter unrelated in-process hosts or tests.
    static STDERR_OVERRIDE: Cell<Option<u8>> = const { Cell::new(None) };
}

pub(crate) struct StderrLevelGuard(Option<u8>);

impl Drop for StderrLevelGuard {
    fn drop(&mut self) {
        STDERR_OVERRIDE.set(self.0);
    }
}

pub(crate) fn override_stderr_level(level: u8) -> StderrLevelGuard {
    StderrLevelGuard(STDERR_OVERRIDE.replace(Some(level)))
}

static LOGGING_STATE: OnceLock<Option<LoggingState>> = OnceLock::new();

/// File logging destination and minimum level.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FileLoggingConfig {
    pub path: PathBuf,
    pub level: LevelFilter,
}

impl FileLoggingConfig {
    /// Creates a file logging destination with the provided minimum level.
    pub fn new(path: PathBuf, level: LevelFilter) -> Self {
        Self { path, level }
    }
}

/// Logging settings derived from CLI startup state.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DeveloperLoggingConfig {
    pub debug_level: u8,
    pub file: Option<FileLoggingConfig>,
}

impl DeveloperLoggingConfig {
    /// Creates developer logging settings with stderr verbosity only.
    pub fn new(debug_level: u8) -> Self {
        Self {
            debug_level,
            file: None,
        }
    }

    /// Replaces the optional file logging destination.
    pub fn with_file(mut self, file: Option<FileLoggingConfig>) -> Self {
        self.file = file;
        self
    }
}

/// Derives the initial developer logging configuration from raw CLI arguments.
#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
fn bootstrap_logging_config(args: &[OsString]) -> DeveloperLoggingConfig {
    DeveloperLoggingConfig::new(crate::cli::invocation::scan_cli_argv(args).invocation.debug)
}

/// Initializes or reloads the process-global developer logging subscriber.
pub fn init_developer_logging(config: DeveloperLoggingConfig) {
    if let Some(state) = LOGGING_STATE
        .get_or_init(|| LoggingState::initialize(&config))
        .as_ref()
    {
        state.apply(&config);
    }
}

/// Installs the default subscriber only if logging has not already been configured.
pub(crate) fn initialize_developer_logging() {
    LOGGING_STATE.get_or_init(|| LoggingState::initialize(&DeveloperLoggingConfig::new(0)));
}

/// Parses a textual log level into a `tracing_subscriber` filter.
///
/// Accepts canonical names such as `error`, `warn`, `info`, `debug`, and `trace`.
pub fn parse_level_filter(value: &str) -> Option<LevelFilter> {
    match value.trim().to_ascii_lowercase().as_str() {
        "error" => Some(LevelFilter::ERROR),
        "warn" | "warning" => Some(LevelFilter::WARN),
        "info" => Some(LevelFilter::INFO),
        "debug" => Some(LevelFilter::DEBUG),
        "trace" => Some(LevelFilter::TRACE),
        _ => None,
    }
}

// Operator events stay compact; internal diagnostics retain the active spans.
struct StderrFormat(Arc<AtomicU8>);

impl<S, N> fmt::FormatEvent<S, N> for StderrFormat
where
    S: tracing::Subscriber + for<'a> registry::LookupSpan<'a>,
    N: for<'a> fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        context: &fmt::FmtContext<'_, S, N>,
        mut writer: fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let _ = &self.0;
        context.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

struct LoggingState {
    stderr_level: Arc<AtomicU8>,
    file_handle: Box<dyn ReloadFilterHandle>,
    file_writer: DynamicFileWriter,
}

trait ReloadFilterHandle: Send + Sync {
    fn reload(&self, level: LevelFilter) -> Result<(), String>;
}

impl<S> ReloadFilterHandle for reload::Handle<LevelFilter, S>
where
    S: tracing::Subscriber + Send + Sync + 'static,
{
    fn reload(&self, level: LevelFilter) -> Result<(), String> {
        reload::Handle::reload(self, level).map_err(|err| err.to_string())
    }
}

impl LoggingState {
    fn initialize(config: &DeveloperLoggingConfig) -> Option<Self> {
        let file_writer = DynamicFileWriter::default();
        let file_level = match file_writer.configure(config.file.as_ref()) {
            Ok(level) => level,
            Err(err) => {
                eprintln!("failed to initialize file logging: {err}");
                LevelFilter::OFF
            }
        };

        let use_ansi = std::io::stderr().is_terminal();
        let stderr_level = Arc::new(AtomicU8::new(config.debug_level));
        let level = stderr_level.clone();
        let stderr_filter = tracing_subscriber::filter::filter_fn(move |metadata| {
            let current = STDERR_OVERRIDE
                .get()
                .unwrap_or_else(|| level.load(Ordering::Relaxed));
            metadata.target() == "osp::diagnostic" && metadata.level() <= &map_debug_level(current)
        })
        .with_max_level_hint(tracing::Level::TRACE);
        let stderr_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(true)
            .with_ansi(use_ansi)
            .event_format(StderrFormat(stderr_level.clone()))
            .with_filter(stderr_filter);
        let (file_filter, file_handle) = reload::Layer::new(file_level);
        let file_writer_for_layer = file_writer.clone();
        let file_layer = fmt::layer()
            .with_writer(move || file_writer_for_layer.clone())
            .with_target(true)
            .with_ansi(false)
            .compact()
            .with_filter(file_filter)
            .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
                matches!(metadata.target(), "osp::diagnostic" | "osp::evidence")
            }));

        if let Err(err) = registry()
            .with(crate::startup_trace::layer())
            .with(stderr_layer)
            .with(file_layer)
            .try_init()
        {
            if config.debug_level >= 2 {
                eprintln!("logging already initialized: {err}");
            }
            return None;
        }

        Some(Self {
            stderr_level,
            file_handle: Box::new(file_handle),
            file_writer,
        })
    }

    fn apply(&self, config: &DeveloperLoggingConfig) {
        self.stderr_level
            .store(config.debug_level, Ordering::Relaxed);

        let file_level = match self.file_writer.configure(config.file.as_ref()) {
            Ok(level) => level,
            Err(err) => {
                self.file_writer.clear();
                eprintln!("failed to initialize file logging: {err}");
                LevelFilter::OFF
            }
        };

        if let Err(err) = self.file_handle.reload(file_level)
            && config.debug_level >= 2
        {
            eprintln!("failed to reload file logging: {err}");
        }
    }
}

#[derive(Clone, Default)]
struct DynamicFileWriter {
    state: Arc<Mutex<DynamicFileState>>,
}

#[derive(Default)]
struct DynamicFileState {
    file: Option<std::fs::File>,
}

impl DynamicFileWriter {
    fn configure(&self, file: Option<&FileLoggingConfig>) -> Result<LevelFilter, String> {
        let opened = match file {
            Some(file) => Some(open_log_file(&file.path)?),
            None => None,
        };
        let mut state = self
            .state
            .lock()
            .map_err(|_| "log file mutex poisoned".to_string())?;
        state.file = opened;
        Ok(file.map(|value| value.level).unwrap_or(LevelFilter::OFF))
    }

    fn clear(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.file = None;
        }
    }
}

impl Write for DynamicFileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("log file mutex poisoned"))?;
        if let Some(file) = state.file.as_mut() {
            file.write(buf)
        } else {
            Ok(buf.len())
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("log file mutex poisoned"))?;
        if let Some(file) = state.file.as_mut() {
            file.flush()
        } else {
            Ok(())
        }
    }
}

fn open_log_file(path: &Path) -> Result<std::fs::File, String> {
    let (directory, _) = split_file_path(path)?;
    std::fs::create_dir_all(&directory).map_err(|err| err.to_string())?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| err.to_string())
}

fn split_file_path(path: &Path) -> Result<(PathBuf, String), String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("invalid log file path: {}", path.display()))?;

    let directory = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok((directory, file_name))
}

fn map_debug_level(debug_level: u8) -> LevelFilter {
    match debug_level {
        0 => LevelFilter::WARN,
        1 => LevelFilter::INFO,
        2 => LevelFilter::DEBUG,
        _ => LevelFilter::TRACE,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{
        DynamicFileWriter, bootstrap_logging_config, map_debug_level, open_log_file,
        parse_level_filter, split_file_path,
    };
    use std::ffi::OsString;
    use std::io::Write as _;
    use std::path::Path;
    use tracing_subscriber::filter::LevelFilter;

    #[test]
    fn logging_level_parsing_and_debug_scanning_cover_supported_inputs_unit() {
        for (count, expected) in [
            (0, LevelFilter::WARN),
            (1, LevelFilter::INFO),
            (2, LevelFilter::DEBUG),
            (3, LevelFilter::TRACE),
            (9, LevelFilter::TRACE),
        ] {
            assert_eq!(map_debug_level(count), expected);
        }

        for (value, expected) in [
            ("warn", Some(LevelFilter::WARN)),
            ("WARNING", Some(LevelFilter::WARN)),
            ("info", Some(LevelFilter::INFO)),
            (" error ", Some(LevelFilter::ERROR)),
            ("debug", Some(LevelFilter::DEBUG)),
            ("trace", Some(LevelFilter::TRACE)),
            ("bad", None),
        ] {
            assert_eq!(parse_level_filter(value), expected);
        }

        let counted = bootstrap_logging_config(&[
            OsString::from("osp"),
            OsString::from("-dd"),
            OsString::from("plugins"),
            OsString::from("--debug"),
            OsString::from("list"),
        ]);
        assert_eq!(counted.debug_level, 3);
        assert!(counted.file.is_none());

        let stopped = bootstrap_logging_config(&[
            OsString::from("osp"),
            OsString::from("-d"),
            OsString::from("--"),
            OsString::from("--debug"),
            OsString::from("-dd"),
        ]);
        assert_eq!(stopped.debug_level, 1);

        let mixed = bootstrap_logging_config(&[
            OsString::from("osp"),
            OsString::from("-vqdd"),
            OsString::from("doctor"),
        ]);
        assert_eq!(mixed.debug_level, 2);
    }

    #[cfg_attr(miri, ignore = "file logging filesystem integration test")]
    #[test]
    fn file_logging_helpers_and_writer_lifecycle_cover_split_open_toggle_and_sink_unit() {
        assert!(split_file_path(Path::new("")).is_err());

        let (bare_dir, bare_name) =
            split_file_path(Path::new("osp.log")).expect("bare file name should be accepted");
        assert!(bare_dir.as_os_str().is_empty());
        assert_eq!(bare_name, "osp.log");

        let (nested_dir, nested_name) = split_file_path(Path::new("logs/osp.log"))
            .expect("nested file path should be accepted");
        assert_eq!(nested_dir, Path::new("logs"));
        assert_eq!(nested_name, "osp.log");

        let dir = make_temp_dir("osp-cli-logging-open");
        let nested_path = dir.join("nested").join("osp.log");
        let _file = open_log_file(&nested_path).expect("log file should open");
        assert!(nested_path.exists());

        let path = dir.join("writer.log");
        let config = super::FileLoggingConfig::new(path.clone(), LevelFilter::INFO);
        let mut writer = DynamicFileWriter::default();
        assert_eq!(
            writer
                .configure(Some(&config))
                .expect("file logging should configure"),
            LevelFilter::INFO
        );
        writer.write_all(b"hello").expect("write should succeed");
        writer.flush().expect("flush should succeed");
        assert_eq!(
            std::fs::read_to_string(&path).expect("log file should exist"),
            "hello"
        );

        writer.clear();
        writer
            .write_all(b"discarded")
            .expect("sink writes should succeed");
        writer.flush().expect("sink flush should succeed");
        assert_eq!(
            std::fs::read_to_string(&path).expect("log file should still exist"),
            "hello"
        );

        let mut sink = DynamicFileWriter::default();
        assert_eq!(
            sink.configure(None).expect("sink configure should succeed"),
            LevelFilter::OFF
        );
        sink.write_all(b"discarded")
            .expect("sink write should succeed");
        sink.flush().expect("sink flush should succeed");
    }

    fn make_temp_dir(prefix: &str) -> crate::tests::TestTempDir {
        crate::tests::make_temp_dir(prefix)
    }
}
