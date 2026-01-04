//! Structured Logging with Tracing

use crate::{Result, TelemetryError};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tracing::Level;
use tracing_subscriber::EnvFilter;

/// Log levels compatible with tracing
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// Parse log level from string
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "trace" => Ok(LogLevel::Trace),
            "debug" => Ok(LogLevel::Debug),
            "info" => Ok(LogLevel::Info),
            "warn" | "warning" => Ok(LogLevel::Warn),
            "error" => Ok(LogLevel::Error),
            _ => Err(TelemetryError::Logging(format!("Invalid log level: {}", s))),
        }
    }

    /// Convert to tracing Level
    pub fn to_tracing_level(&self) -> Level {
        match self {
            LogLevel::Trace => Level::TRACE,
            LogLevel::Debug => Level::DEBUG,
            LogLevel::Info => Level::INFO,
            LogLevel::Warn => Level::WARN,
            LogLevel::Error => Level::ERROR,
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                LogLevel::Trace => "trace",
                LogLevel::Debug => "debug",
                LogLevel::Info => "info",
                LogLevel::Warn => "warn",
                LogLevel::Error => "error",
            }
        )
    }
}

impl FromStr for LogLevel {
    type Err = TelemetryError;
    fn from_str(s: &str) -> Result<Self> {
        LogLevel::from_str(s)
    }
}

/// Output format for logs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogFormat {
    Json,
    Pretty,
    Compact,
}

/// Output destination for logs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogOutput {
    Stdout,
    Stderr,
    File(PathBuf),
    Both { stdout: bool, file: PathBuf },
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    pub level: LogLevel,
    pub format: LogFormat,
    pub output: LogOutput,
    pub include_target: bool,
    pub include_file_line: bool,
    pub include_thread_id: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            format: LogFormat::Pretty,
            output: LogOutput::Stdout,
            include_target: true,
            include_file_line: false,
            include_thread_id: false,
        }
    }
}

/// Contextual information for logging
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogContext {
    pub component: String,
    pub transfer_id: Option<String>,
    pub edge_id: Option<String>,
    pub extra: HashMap<String, String>,
}

impl LogContext {
    pub fn new(component: impl Into<String>) -> Self {
        Self {
            component: component.into(),
            transfer_id: None,
            edge_id: None,
            extra: HashMap::new(),
        }
    }

    pub fn with_transfer_id(mut self, id: impl Into<String>) -> Self {
        self.transfer_id = Some(id.into());
        self
    }

    pub fn with_edge_id(mut self, id: impl Into<String>) -> Self {
        self.edge_id = Some(id.into());
        self
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra.insert(key.into(), value.into());
        self
    }
}

/// Structured logger
pub struct StructuredLogger {
    config: LogConfig,
    context: Arc<Mutex<Option<LogContext>>>,
}

impl StructuredLogger {
    pub fn new(config: LogConfig) -> Result<Self> {
        Ok(Self {
            config,
            context: Arc::new(Mutex::new(None)),
        })
    }

    pub fn init(&self) -> Result<()> {
        init_logging(&self.config)
    }

    pub fn with_context(&self, ctx: LogContext) -> Self {
        Self {
            config: self.config.clone(),
            context: Arc::new(Mutex::new(Some(ctx))),
        }
    }

    pub fn trace(&self, msg: &str) {
        self.log(LogLevel::Trace, msg);
    }

    pub fn debug(&self, msg: &str) {
        self.log(LogLevel::Debug, msg);
    }

    pub fn info(&self, msg: &str) {
        self.log(LogLevel::Info, msg);
    }

    pub fn warn(&self, msg: &str) {
        self.log(LogLevel::Warn, msg);
    }

    pub fn error(&self, msg: &str) {
        self.log(LogLevel::Error, msg);
    }

    fn log(&self, level: LogLevel, msg: &str) {
        let ctx = self.context.lock();
        if let Some(ref context) = *ctx {
            log_with_context(level, msg, context);
        } else {
            log_plain(level, msg);
        }
    }

    pub fn event(&self, level: LogLevel, msg: &str, fields: &[(&str, &str)]) {
        let ctx = self.context.lock();
        if let Some(ref context) = *ctx {
            log_event_with_context(level, msg, fields, context);
        } else {
            log_event_plain(level, msg, fields);
        }
    }
}

fn log_with_context(level: LogLevel, msg: &str, ctx: &LogContext) {
    match level {
        LogLevel::Trace => tracing::trace!(
            component = ctx.component,
            transfer_id = ctx.transfer_id.as_deref(),
            edge_id = ctx.edge_id.as_deref(),
            "{}",
            msg
        ),
        LogLevel::Debug => tracing::debug!(
            component = ctx.component,
            transfer_id = ctx.transfer_id.as_deref(),
            edge_id = ctx.edge_id.as_deref(),
            "{}",
            msg
        ),
        LogLevel::Info => tracing::info!(
            component = ctx.component,
            transfer_id = ctx.transfer_id.as_deref(),
            edge_id = ctx.edge_id.as_deref(),
            "{}",
            msg
        ),
        LogLevel::Warn => tracing::warn!(
            component = ctx.component,
            transfer_id = ctx.transfer_id.as_deref(),
            edge_id = ctx.edge_id.as_deref(),
            "{}",
            msg
        ),
        LogLevel::Error => tracing::error!(
            component = ctx.component,
            transfer_id = ctx.transfer_id.as_deref(),
            edge_id = ctx.edge_id.as_deref(),
            "{}",
            msg
        ),
    }
}

fn log_plain(level: LogLevel, msg: &str) {
    match level {
        LogLevel::Trace => tracing::trace!("{}", msg),
        LogLevel::Debug => tracing::debug!("{}", msg),
        LogLevel::Info => tracing::info!("{}", msg),
        LogLevel::Warn => tracing::warn!("{}", msg),
        LogLevel::Error => tracing::error!("{}", msg),
    }
}

fn log_event_with_context(level: LogLevel, msg: &str, fields: &[(&str, &str)], ctx: &LogContext) {
    match level {
        LogLevel::Trace => tracing::trace!(
            component = ctx.component, transfer_id = ctx.transfer_id.as_deref(),
            edge_id = ctx.edge_id.as_deref(), fields = ?fields, "{}", msg
        ),
        LogLevel::Debug => tracing::debug!(
            component = ctx.component, transfer_id = ctx.transfer_id.as_deref(),
            edge_id = ctx.edge_id.as_deref(), fields = ?fields, "{}", msg
        ),
        LogLevel::Info => tracing::info!(
            component = ctx.component, transfer_id = ctx.transfer_id.as_deref(),
            edge_id = ctx.edge_id.as_deref(), fields = ?fields, "{}", msg
        ),
        LogLevel::Warn => tracing::warn!(
            component = ctx.component, transfer_id = ctx.transfer_id.as_deref(),
            edge_id = ctx.edge_id.as_deref(), fields = ?fields, "{}", msg
        ),
        LogLevel::Error => tracing::error!(
            component = ctx.component, transfer_id = ctx.transfer_id.as_deref(),
            edge_id = ctx.edge_id.as_deref(), fields = ?fields, "{}", msg
        ),
    }
}

fn log_event_plain(level: LogLevel, msg: &str, fields: &[(&str, &str)]) {
    match level {
        LogLevel::Trace => tracing::trace!(fields = ?fields, "{}", msg),
        LogLevel::Debug => tracing::debug!(fields = ?fields, "{}", msg),
        LogLevel::Info => tracing::info!(fields = ?fields, "{}", msg),
        LogLevel::Warn => tracing::warn!(fields = ?fields, "{}", msg),
        LogLevel::Error => tracing::error!(fields = ?fields, "{}", msg),
    }
}

/// Builder for structured logs
pub struct LogBuilder {
    level: LogLevel,
    message: Option<String>,
    fields: Vec<(String, String)>,
    context: Option<LogContext>,
}

impl LogBuilder {
    pub fn new(level: LogLevel) -> Self {
        Self {
            level,
            message: None,
            fields: Vec::new(),
            context: None,
        }
    }

    pub fn message(mut self, msg: &str) -> Self {
        self.message = Some(msg.to_string());
        self
    }

    pub fn field(mut self, key: &str, value: impl ToString) -> Self {
        self.fields.push((key.to_string(), value.to_string()));
        self
    }

    pub fn context(mut self, ctx: &LogContext) -> Self {
        self.context = Some(ctx.clone());
        self
    }

    pub fn emit(self) {
        let msg = self.message.unwrap_or_default();
        if let Some(ref ctx) = self.context {
            log_event_with_context(
                self.level,
                &msg,
                &self
                    .fields
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect::<Vec<_>>(),
                ctx,
            );
        } else {
            log_event_plain(
                self.level,
                &msg,
                &self
                    .fields
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect::<Vec<_>>(),
            );
        }
    }
}

/// RAII guard for span context
pub struct LogGuard {
    _span: tracing::span::Entered<'static>,
    start: std::time::Instant,
    name: String,
}

impl LogGuard {
    fn new(span: tracing::Span, name: String) -> Self {
        let start = std::time::Instant::now();
        let static_span = Box::leak(Box::new(span));
        Self {
            _span: static_span.enter(),
            start,
            name,
        }
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.start.elapsed()
    }
}

impl Drop for LogGuard {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        tracing::debug!(
            span = self.name.as_str(),
            duration_ms = duration.as_millis() as u64,
            "span completed"
        );
    }
}

/// Builder for tracing spans
pub struct SpanBuilder {
    name: String,
    level: LogLevel,
    fields: Vec<(String, String)>,
}

impl SpanBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            level: LogLevel::Debug,
            fields: Vec::new(),
        }
    }

    pub fn level(mut self, level: LogLevel) -> Self {
        self.level = level;
        self
    }

    pub fn field(mut self, key: &str, value: impl ToString) -> Self {
        self.fields.push((key.to_string(), value.to_string()));
        self
    }

    pub fn enter(self) -> LogGuard {
        let span = match self.level {
            LogLevel::Trace => tracing::trace_span!("span", name = %self.name),
            LogLevel::Debug => tracing::debug_span!("span", name = %self.name),
            LogLevel::Info => tracing::info_span!("span", name = %self.name),
            LogLevel::Warn => tracing::warn_span!("span", name = %self.name),
            LogLevel::Error => tracing::error_span!("span", name = %self.name),
        };
        for (key, value) in &self.fields {
            span.record(key.as_str(), value.as_str());
        }
        LogGuard::new(span, self.name)
    }
}

/// Initialize global logging with the given configuration
pub fn init_logging(config: &LogConfig) -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(config.level.to_string()));

    match &config.output {
        LogOutput::Stdout => apply_fmt(config, std::io::stdout, filter)?,
        LogOutput::Stderr => apply_fmt(config, std::io::stderr, filter)?,
        LogOutput::File(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            apply_fmt(config, move || file.try_clone().unwrap(), filter)?;
        }
        LogOutput::Both { stdout, file } => {
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let file_handle = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(file)?;
            if *stdout {
                let combined = CombinedWriter {
                    stdout: Arc::new(Mutex::new(std::io::stdout())),
                    file: Arc::new(Mutex::new(file_handle)),
                };
                apply_fmt(config, combined, filter)?;
            } else {
                apply_fmt(config, move || file_handle.try_clone().unwrap(), filter)?;
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
struct CombinedWriter {
    stdout: Arc<Mutex<std::io::Stdout>>,
    file: Arc<Mutex<std::fs::File>>,
}

impl Write for CombinedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.stdout.lock().write_all(buf)?;
        self.file.lock().write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stdout.lock().flush()?;
        self.file.lock().flush()?;
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CombinedWriter {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn apply_fmt<W>(config: &LogConfig, writer: W, filter: EnvFilter) -> Result<()>
where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    let init_result = match config.format {
        LogFormat::Json => tracing_subscriber::fmt()
            .json()
            .with_writer(writer)
            .with_target(config.include_target)
            .with_file(config.include_file_line)
            .with_line_number(config.include_file_line)
            .with_thread_ids(config.include_thread_id)
            .with_env_filter(filter)
            .try_init(),
        LogFormat::Pretty => tracing_subscriber::fmt()
            .pretty()
            .with_writer(writer)
            .with_target(config.include_target)
            .with_file(config.include_file_line)
            .with_line_number(config.include_file_line)
            .with_thread_ids(config.include_thread_id)
            .with_env_filter(filter)
            .try_init(),
        LogFormat::Compact => tracing_subscriber::fmt()
            .compact()
            .with_writer(writer)
            .with_target(config.include_target)
            .with_file(config.include_file_line)
            .with_line_number(config.include_file_line)
            .with_thread_ids(config.include_thread_id)
            .with_env_filter(filter)
            .try_init(),
    };
    init_result.map_err(|e| TelemetryError::Init(format!("Failed to init subscriber: {}", e)))
}

#[cfg(test)]
mod tests;
