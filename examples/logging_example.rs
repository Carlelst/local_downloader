/// 日志系统使用示例
///
/// 演示如何使用新的结构化日志系统
use anyhow::Result;
use video_processor::{
    log_context, LogFormat, LogLevel, LogOutput, LogRotationConfig, LoggerConfig, SimpleLogger,
};

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== 视频处理器日志系统示例 ===\n");

    // 创建临时目录用于日志文件
    let temp_dir = std::env::temp_dir().join("video_processor_logs");
    std::fs::create_dir_all(&temp_dir)?;

    println!("1. 控制台日志记录器示例");

    // 示例1: 控制台日志记录器
    let console_config = LoggerConfig {
        level: LogLevel::Info,
        output: LogOutput::Stdout,
        format: LogFormat::Formatted,
        ..LoggerConfig::default()
    };

    let console_logger = SimpleLogger::new(console_config)?;

    // 使用结构化日志
    let context = log_context! {
        "user_id" => 12345,
        "action" => "download_video",
        "url" => "https://example.com/video.m3u8"
    };

    console_logger.info("video_processor", "这是一条信息日志", context.clone())?;
    console_logger.warn("video_processor", "这是一条警告日志", context.clone())?;
    console_logger.error("video_processor", "这是一条错误日志", context)?;

    println!("\n2. JSON格式日志示例");

    // 示例2: JSON格式日志
    let json_config = LoggerConfig {
        level: LogLevel::Debug,
        output: LogOutput::Stdout,
        format: LogFormat::Json,
        ..LoggerConfig::default()
    };

    let json_logger = SimpleLogger::new(json_config)?;

    let context = log_context! {
        "user_id" => 12345,
        "action" => "download_video",
        "url" => "https://example.com/video.m3u8",
        "segments_count" => 150
    };

    json_logger.info("video_processor", "开始下载视频", context)?;

    let error_context = log_context! {
        "error_code" => "NETWORK_TIMEOUT",
        "retry_count" => 3,
        "segment_index" => 42
    };

    json_logger.error("download_manager", "片段下载失败", error_context)?;

    println!("\n3. 文件日志示例");

    let log_file = temp_dir.join("app.log");
    let file_config = LoggerConfig {
        level: LogLevel::Debug,
        output: LogOutput::File(log_file.clone()),
        format: LogFormat::Json,
        ..LoggerConfig::default()
    };

    let file_logger = SimpleLogger::new(file_config)?;

    // 记录一些日志到文件
    file_logger.info("app", "应用程序启动", log_context! {"version" => "1.0.0"})?;
    file_logger.debug(
        "config",
        "配置已加载",
        log_context! {"max_concurrent_downloads" => 8},
    )?;
    file_logger.warn(
        "network",
        "检测到网络延迟较高",
        log_context! {"latency_ms" => 500},
    )?;

    println!("日志已写入文件: {:?}", log_file);

    println!("\n4. 混合日志示例（同时输出到控制台和文件）");

    let hybrid_log_file = temp_dir.join("hybrid.log");
    let hybrid_config = LoggerConfig {
        level: LogLevel::Info,
        output: LogOutput::Multiple(vec![
            LogOutput::Stdout,
            LogOutput::File(hybrid_log_file.clone()),
        ]),
        format: LogFormat::Formatted,
        ..LoggerConfig::default()
    };

    let hybrid_logger = SimpleLogger::new(hybrid_config)?;

    hybrid_logger.info(
        "hybrid",
        "这条日志会同时出现在控制台和文件中",
        log_context! {},
    )?;
    hybrid_logger.error("hybrid", "混合日志记录器测试完成", log_context! {})?;

    println!("混合日志文件: {:?}", hybrid_log_file);

    println!("\n5. 日志轮转示例");

    let rotation_config = LogRotationConfig {
        max_file_size: 1024, // 1KB (很小，用于演示)
        max_files: 3,
        enabled: true,
    };

    let rotation_logger_config = LoggerConfig {
        level: LogLevel::Debug,
        output: LogOutput::File(temp_dir.join("rotation.log")),
        format: LogFormat::Formatted,
        rotation: rotation_config,
        ..LoggerConfig::default()
    };

    let rotation_logger = SimpleLogger::new(rotation_logger_config)?;

    // 生成足够的日志来触发轮转
    for i in 0..50 {
        let context = log_context! {
            "iteration" => i,
            "message_length" => 100
        };
        rotation_logger.info(
            "rotation_test",
            &format!(
                "这是第{}条日志消息，用于测试日志轮转功能。这条消息比较长，用来快速填满日志文件。",
                i
            ),
            context,
        )?;
    }

    println!("日志轮转测试完成，检查 {:?} 目录", temp_dir);

    // 列出生成的日志文件
    if let Ok(entries) = std::fs::read_dir(&temp_dir) {
        println!("\n生成的日志文件:");
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        println!("  {} ({} 字节)", path.display(), metadata.len());
                    }
                }
            }
        }
    }

    println!("\n6. 带追踪ID的日志示例");

    let trace_config = LoggerConfig {
        level: LogLevel::Info,
        output: LogOutput::Stdout,
        format: LogFormat::Formatted,
        ..LoggerConfig::default()
    };

    let trace_logger = SimpleLogger::new(trace_config)?;

    // 创建带追踪ID的日志条目
    use video_processor::LogEntry;
    let trace_entry = LogEntry::new(
        LogLevel::Info,
        "request_handler".to_string(),
        "处理用户请求".to_string(),
        log_context! {
            "user_id" => 12345,
            "request_id" => "req_abc123"
        },
        Some("trace-xyz789".to_string()),
    );

    trace_logger.log_entry(trace_entry)?;

    println!("\n=== 示例完成 ===");
    println!("提示: 检查生成的日志文件以查看不同格式的日志输出");

    Ok(())
}
