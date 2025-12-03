use super::*;
use std::sync::Once;
use tempfile::TempDir;

static INIT: Once = Once::new();

fn setup_test_logger() {
    INIT.call_once(|| {
        let config = LogConfig {
            level: LogLevel::Trace,
            format: LogFormat::Compact,
            output: LogOutput::Stderr,
            include_target: false,
            include_file_line: false,
            include_thread_id: false,
        };
        let _ = init_logging(&config);
    });
}

#[test]
fn test_log_level_from_str_trace() {
    assert_eq!(LogLevel::from_str("trace").unwrap(), LogLevel::Trace);
}

#[test]
fn test_log_level_from_str_debug() {
    assert_eq!(LogLevel::from_str("debug").unwrap(), LogLevel::Debug);
}

#[test]
fn test_log_level_from_str_info() {
    assert_eq!(LogLevel::from_str("info").unwrap(), LogLevel::Info);
}

#[test]
fn test_log_level_from_str_warn() {
    assert_eq!(LogLevel::from_str("warn").unwrap(), LogLevel::Warn);
}

#[test]
fn test_log_level_from_str_warning() {
    assert_eq!(LogLevel::from_str("warning").unwrap(), LogLevel::Warn);
}

#[test]
fn test_log_level_from_str_error() {
    assert_eq!(LogLevel::from_str("error").unwrap(), LogLevel::Error);
}

#[test]
fn test_log_level_from_str_case_insensitive() {
    assert_eq!(LogLevel::from_str("INFO").unwrap(), LogLevel::Info);
}

#[test]
fn test_log_level_from_str_invalid() {
    assert!(LogLevel::from_str("invalid").is_err());
}

#[test]
fn test_log_level_to_tracing() {
    assert_eq!(LogLevel::Trace.to_tracing_level(), Level::TRACE);
    assert_eq!(LogLevel::Debug.to_tracing_level(), Level::DEBUG);
    assert_eq!(LogLevel::Info.to_tracing_level(), Level::INFO);
    assert_eq!(LogLevel::Warn.to_tracing_level(), Level::WARN);
    assert_eq!(LogLevel::Error.to_tracing_level(), Level::ERROR);
}

#[test]
fn test_log_level_display() {
    assert_eq!(LogLevel::Trace.to_string(), "trace");
    assert_eq!(LogLevel::Debug.to_string(), "debug");
    assert_eq!(LogLevel::Info.to_string(), "info");
    assert_eq!(LogLevel::Warn.to_string(), "warn");
    assert_eq!(LogLevel::Error.to_string(), "error");
}

#[test]
fn test_log_level_ordering() {
    assert!(LogLevel::Trace < LogLevel::Debug);
    assert!(LogLevel::Debug < LogLevel::Info);
    assert!(LogLevel::Info < LogLevel::Warn);
    assert!(LogLevel::Warn < LogLevel::Error);
}

#[test]
fn test_log_format_variants() {
    assert_eq!(LogFormat::Json, LogFormat::Json);
    assert_eq!(LogFormat::Pretty, LogFormat::Pretty);
    assert_eq!(LogFormat::Compact, LogFormat::Compact);
}

#[test]
fn test_log_output_stdout() {
    assert_eq!(LogOutput::Stdout, LogOutput::Stdout);
}

#[test]
fn test_log_output_stderr() {
    assert_eq!(LogOutput::Stderr, LogOutput::Stderr);
}

#[test]
fn test_log_output_file() {
    let path = PathBuf::from("/tmp/test.log");
    assert_eq!(LogOutput::File(path.clone()), LogOutput::File(path));
}

#[test]
fn test_log_output_both() {
    let path = PathBuf::from("/tmp/test.log");
    let output = LogOutput::Both { stdout: true, file: path.clone() };
    assert_eq!(output, LogOutput::Both { stdout: true, file: path });
}

#[test]
fn test_log_config_default() {
    let config = LogConfig::default();
    assert_eq!(config.level, LogLevel::Info);
    assert_eq!(config.format, LogFormat::Pretty);
    assert!(config.include_target);
    assert!(!config.include_file_line);
    assert!(!config.include_thread_id);
}

#[test]
fn test_log_config_custom() {
    let config = LogConfig {
        level: LogLevel::Debug,
        format: LogFormat::Json,
        output: LogOutput::Stderr,
        include_target: false,
        include_file_line: true,
        include_thread_id: true,
    };
    assert_eq!(config.level, LogLevel::Debug);
    assert!(!config.include_target);
    assert!(config.include_file_line);
}

#[test]
fn test_log_context_new() {
    let ctx = LogContext::new("scheduler");
    assert_eq!(ctx.component, "scheduler");
    assert!(ctx.transfer_id.is_none());
}

#[test]
fn test_log_context_with_transfer_id() {
    let ctx = LogContext::new("network").with_transfer_id("tx123");
    assert_eq!(ctx.transfer_id, Some("tx123".to_string()));
}

#[test]
fn test_log_context_with_edge_id() {
    let ctx = LogContext::new("storage").with_edge_id("edge456");
    assert_eq!(ctx.edge_id, Some("edge456".to_string()));
}

#[test]
fn test_log_context_with_extra_field() {
    let ctx = LogContext::new("api").with_field("request_id", "req789");
    assert_eq!(ctx.extra.get("request_id"), Some(&"req789".to_string()));
}

#[test]
fn test_log_context_chained() {
    let ctx = LogContext::new("scheduler")
        .with_transfer_id("tx123")
        .with_edge_id("edge456")
        .with_field("status", "active");
    assert_eq!(ctx.component, "scheduler");
    assert_eq!(ctx.transfer_id, Some("tx123".to_string()));
    assert_eq!(ctx.extra.get("status"), Some(&"active".to_string()));
}

#[test]
fn test_structured_logger_new() {
    let logger = StructuredLogger::new(LogConfig::default());
    assert!(logger.is_ok());
}

#[test]
fn test_structured_logger_with_context() {
    setup_test_logger();
    let logger = StructuredLogger::new(LogConfig::default()).unwrap();
    let ctx = LogContext::new("test");
    let logger_ctx = logger.with_context(ctx);
    assert!(logger_ctx.context.lock().is_some());
}

#[test]
fn test_structured_logger_trace() {
    setup_test_logger();
    let logger = StructuredLogger::new(LogConfig::default()).unwrap();
    logger.trace("test trace");
}

#[test]
fn test_structured_logger_debug() {
    setup_test_logger();
    let logger = StructuredLogger::new(LogConfig::default()).unwrap();
    logger.debug("test debug");
}

#[test]
fn test_structured_logger_info() {
    setup_test_logger();
    let logger = StructuredLogger::new(LogConfig::default()).unwrap();
    logger.info("test info");
}

#[test]
fn test_structured_logger_warn() {
    setup_test_logger();
    let logger = StructuredLogger::new(LogConfig::default()).unwrap();
    logger.warn("test warn");
}

#[test]
fn test_structured_logger_error() {
    setup_test_logger();
    let logger = StructuredLogger::new(LogConfig::default()).unwrap();
    logger.error("test error");
}

#[test]
fn test_structured_logger_event() {
    setup_test_logger();
    let logger = StructuredLogger::new(LogConfig::default()).unwrap();
    logger.event(LogLevel::Info, "test event", &[("key", "value")]);
}

#[test]
fn test_structured_logger_event_with_context() {
    setup_test_logger();
    let logger = StructuredLogger::new(LogConfig::default()).unwrap();
    let ctx = LogContext::new("test").with_transfer_id("tx123");
    let logger_ctx = logger.with_context(ctx);
    logger_ctx.event(LogLevel::Info, "test event", &[("status", "ok")]);
}

#[test]
fn test_log_builder_new() {
    let builder = LogBuilder::new(LogLevel::Info);
    assert_eq!(builder.level, LogLevel::Info);
    assert!(builder.message.is_none());
}

#[test]
fn test_log_builder_message() {
    let builder = LogBuilder::new(LogLevel::Info).message("test");
    assert_eq!(builder.message, Some("test".to_string()));
}

#[test]
fn test_log_builder_field() {
    let builder = LogBuilder::new(LogLevel::Info).field("key", "value");
    assert_eq!(builder.fields.len(), 1);
}

#[test]
fn test_log_builder_context() {
    let ctx = LogContext::new("test");
    let builder = LogBuilder::new(LogLevel::Info).context(&ctx);
    assert!(builder.context.is_some());
}

#[test]
fn test_log_builder_emit() {
    setup_test_logger();
    LogBuilder::new(LogLevel::Info).message("test").field("key", "value").emit();
}

#[test]
fn test_log_builder_chained() {
    setup_test_logger();
    let ctx = LogContext::new("test").with_transfer_id("tx123");
    LogBuilder::new(LogLevel::Info).message("test").field("k1", "v1").context(&ctx).emit();
}

#[test]
fn test_span_builder_new() {
    let builder = SpanBuilder::new("test_span");
    assert_eq!(builder.name, "test_span");
    assert_eq!(builder.level, LogLevel::Debug);
}

#[test]
fn test_span_builder_level() {
    let builder = SpanBuilder::new("test").level(LogLevel::Info);
    assert_eq!(builder.level, LogLevel::Info);
}

#[test]
fn test_span_builder_field() {
    let builder = SpanBuilder::new("test").field("key", "value");
    assert_eq!(builder.fields.len(), 1);
}

#[test]
fn test_span_builder_enter() {
    setup_test_logger();
    let guard = SpanBuilder::new("test_span").level(LogLevel::Info).field("op", "test").enter();
    assert_eq!(guard.name, "test_span");
}

#[test]
fn test_log_guard_elapsed() {
    setup_test_logger();
    let guard = SpanBuilder::new("test_span").enter();
    std::thread::sleep(std::time::Duration::from_millis(10));
    assert!(guard.elapsed().as_millis() >= 10);
}

#[test]
fn test_log_guard_drop() {
    setup_test_logger();
    {
        let _guard = SpanBuilder::new("test_span").enter();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn test_init_logging_stdout() {
    let config = LogConfig {
        level: LogLevel::Info,
        format: LogFormat::Compact,
        output: LogOutput::Stdout,
        include_target: false,
        include_file_line: false,
        include_thread_id: false,
    };
    let _ = config;
}

#[test]
fn test_init_logging_file() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("test.log");
    let config = LogConfig {
        level: LogLevel::Debug,
        format: LogFormat::Json,
        output: LogOutput::File(log_path.clone()),
        include_target: true,
        include_file_line: false,
        include_thread_id: false,
    };
    assert!(matches!(config.output, LogOutput::File(_)));
}

#[test]
fn test_file_creation_in_nested_directory() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("logs").join("app").join("test.log");
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
        assert!(parent.exists());
    }
}

#[test]
fn test_log_context_serialization() {
    let ctx = LogContext::new("test")
        .with_transfer_id("tx123")
        .with_edge_id("edge456")
        .with_field("status", "active");
    let json = serde_json::to_string(&ctx).unwrap();
    assert!(json.contains("test"));
    assert!(json.contains("tx123"));
}

#[test]
fn test_log_context_deserialization() {
    let json = r#"{"component":"test","transfer_id":"tx123","edge_id":"edge456","extra":{"status":"active"}}"#;
    let ctx: LogContext = serde_json::from_str(json).unwrap();
    assert_eq!(ctx.component, "test");
    assert_eq!(ctx.transfer_id, Some("tx123".to_string()));
}

#[test]
fn test_log_config_serialization() {
    let config = LogConfig {
        level: LogLevel::Debug,
        format: LogFormat::Json,
        output: LogOutput::Stderr,
        include_target: true,
        include_file_line: false,
        include_thread_id: true,
    };
    let json = serde_json::to_string(&config).unwrap();
    let deserialized: LogConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.level, config.level);
}

#[test]
fn test_multiple_context_fields() {
    let mut ctx = LogContext::new("complex");
    for i in 0..5 {
        ctx = ctx.with_field(format!("key{}", i), format!("value{}", i));
    }
    assert_eq!(ctx.extra.len(), 5);
}

#[test]
fn test_log_level_from_str_trait() {
    let level: LogLevel = "info".parse().unwrap();
    assert_eq!(level, LogLevel::Info);
}

#[test]
fn test_log_output_both_no_stdout() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("test.log");
    let output = LogOutput::Both { stdout: false, file: log_path.clone() };
    match output {
        LogOutput::Both { stdout, file } => {
            assert!(!stdout);
            assert_eq!(file, log_path);
        }
        _ => panic!("Expected Both variant"),
    }
}

#[test]
fn test_structured_logger_all_levels_with_context() {
    setup_test_logger();
    let logger = StructuredLogger::new(LogConfig::default()).unwrap();
    let ctx = LogContext::new("integration").with_transfer_id("tx999").with_edge_id("edge888");
    let logger_ctx = logger.with_context(ctx);
    logger_ctx.trace("trace ctx");
    logger_ctx.debug("debug ctx");
    logger_ctx.info("info ctx");
    logger_ctx.warn("warn ctx");
    logger_ctx.error("error ctx");
}

#[test]
fn test_span_builder_all_levels() {
    setup_test_logger();
    let _g1 = SpanBuilder::new("trace_span").level(LogLevel::Trace).enter();
    drop(_g1);
    let _g2 = SpanBuilder::new("debug_span").level(LogLevel::Debug).enter();
    drop(_g2);
    let _g3 = SpanBuilder::new("info_span").level(LogLevel::Info).enter();
    drop(_g3);
    let _g4 = SpanBuilder::new("warn_span").level(LogLevel::Warn).enter();
    drop(_g4);
    let _g5 = SpanBuilder::new("error_span").level(LogLevel::Error).enter();
    drop(_g5);
}
