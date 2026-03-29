//! # Video Processor API Usage Examples
//!
//! This file contains comprehensive examples demonstrating how to use the Video Processor library.
//! Each example shows different aspects of the API and best practices for common use cases.

use std::time::Duration;
use tokio::time::sleep;
use video_processor::{
    create_default_service_factory, init_console_logger, load_config, AdaptiveDownloadManager,
    Config as DomainConfig, DefaultFFmpegHandler, DefaultM3U8Parser, DownloadStats, LogLevel,
    ProcessingResult, SystemResourceMonitor, VideoProcessorService,
};

/// Example 1: Basic video processing workflow
///
/// This example demonstrates the most common use case: loading configuration
/// and processing M3U8 streams using the high-level VideoProcessorService.
async fn basic_video_processing() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Basic Video Processing Example ===");

    // Initialize logging
    init_console_logger(LogLevel::Info)?;

    // Load configuration from config.yaml
    let config = load_config()?;
    println!("Loaded configuration with {} URLs", config.m3u8_urls.len());

    // Create service factory with default configuration
    let factory = create_default_service_factory().await?;

    // Get the video processor service
    let processor = factory.video_processor_service().await?;

    // Process each M3U8 URL
    for (index, url) in config.m3u8_urls.iter().enumerate() {
        println!("Processing URL {}: {}", index + 1, url);

        match processor
            .process_stream(url, &config.output_directory)
            .await
        {
            Ok(result) => {
                println!("✓ Successfully processed: {:?}", result);
            }
            Err(e) => {
                eprintln!("✗ Failed to process {}: {}", url, e);
            }
        }
    }

    Ok(())
}

/// Example 2: Advanced download management with progress monitoring
///
/// This example shows how to use the AdaptiveDownloadManager directly
/// for more control over the download process and progress monitoring.
async fn advanced_download_management() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Advanced Download Management Example ===");

    // Create download manager with custom configuration
    let download_config = video_processor::DownloadConfig {
        max_concurrent_downloads: 8,
        retry_attempts: 5,
        timeout_seconds: 60,
        adaptive_concurrency: true,
        bandwidth_monitoring: true,
    };

    let download_manager = AdaptiveDownloadManager::new(download_config);

    // Example URLs (replace with actual M3U8 segment URLs)
    let segment_urls = vec![
        "https://example.com/segment1.ts".to_string(),
        "https://example.com/segment2.ts".to_string(),
        "https://example.com/segment3.ts".to_string(),
    ];

    println!("Starting download of {} segments", segment_urls.len());

    // Start download in background and monitor progress
    let download_handle =
        tokio::spawn(async move { download_manager.download_segments(segment_urls).await });

    // Monitor progress while downloading
    loop {
        let stats = download_manager.get_download_stats();
        println!(
            "Progress: {}/{} segments, Speed: {:.2} MB/s, ETA: {:?}",
            stats.completed_segments,
            stats.total_segments,
            stats.download_speed / 1_000_000.0, // Convert to MB/s
            stats.estimated_time_remaining
        );

        if stats.completed_segments >= stats.total_segments {
            break;
        }

        sleep(Duration::from_secs(1)).await;
    }

    // Wait for download completion
    let segments = download_handle.await??;
    println!("✓ Downloaded {} segments successfully", segments.len());

    Ok(())
}

/// Example 3: M3U8 parsing and analysis
///
/// This example demonstrates how to parse M3U8 playlists and analyze
/// their content before processing.
async fn m3u8_parsing_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== M3U8 Parsing Example ===");

    let parser = DefaultM3U8Parser::new();
    let m3u8_url = "https://example.com/playlist.m3u8";

    // Parse the M3U8 playlist
    println!("Parsing M3U8 from: {}", m3u8_url);
    let playlist = parser.parse(m3u8_url).await?;

    println!("Playlist metadata:");
    println!("  - Total segments: {}", playlist.segments.len());
    println!("  - Encryption: {:?}", playlist.encryption.is_some());

    if let Some(encryption) = &playlist.encryption {
        println!("  - Encryption method: {:?}", encryption.method);
        println!("  - Key URL: {:?}", encryption.key_url);
    }

    // Resolve segment URLs
    let segments = parser.resolve_segments(&playlist).await?;

    // Analyze segments
    let total_duration: f64 = segments.iter().map(|s| s.duration).sum();
    let encrypted_count = segments.iter().filter(|s| s.encryption.is_some()).count();

    println!("Segment analysis:");
    println!("  - Total duration: {:.2} seconds", total_duration);
    println!(
        "  - Encrypted segments: {}/{}",
        encrypted_count,
        segments.len()
    );

    // Show first few segments
    println!("First 3 segments:");
    for (i, segment) in segments.iter().take(3).enumerate() {
        println!("  {}. {} ({}s)", i + 1, segment.url, segment.duration);
    }

    Ok(())
}

/// Example 4: FFmpeg integration and hardware acceleration
///
/// This example shows how to use the FFmpeg handler for video processing
/// with hardware acceleration detection and optimization.
async fn ffmpeg_integration_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== FFmpeg Integration Example ===");

    let ffmpeg_handler = DefaultFFmpegHandler::new();

    // Detect available hardware acceleration
    println!("Detecting hardware acceleration...");
    let accelerators = ffmpeg_handler.detect_hardware_acceleration().await;

    if accelerators.is_empty() {
        println!("No hardware acceleration available, using software encoding");
    } else {
        println!("Available hardware accelerators:");
        for accelerator in &accelerators {
            println!("  - {:?}", accelerator);
        }
    }

    // Create processing configuration
    let processing_config = video_processor::ProcessingConfig {
        output_format: "mp4".to_string(),
        video_codec: if !accelerators.is_empty() {
            "h264_nvenc".to_string() // Use NVIDIA hardware encoding if available
        } else {
            "libx264".to_string() // Fallback to software encoding
        },
        audio_codec: "aac".to_string(),
        video_bitrate: Some("2M".to_string()),
        audio_bitrate: Some("128k".to_string()),
        use_hardware_acceleration: !accelerators.is_empty(),
    };

    // Optimize encoding parameters based on input
    let input_info = video_processor::InputInfo {
        width: 1920,
        height: 1080,
        fps: 30.0,
        duration: 3600.0, // 1 hour
    };

    let optimized_params = ffmpeg_handler.optimize_encoding_params(&input_info).await;
    println!("Optimized encoding parameters: {:?}", optimized_params);

    // Start FFmpeg process (example - would need actual input)
    println!("FFmpeg configuration ready for processing");

    Ok(())
}

/// Example 5: System resource monitoring and adaptive behavior
///
/// This example demonstrates how to monitor system resources and
/// adapt processing behavior accordingly.
async fn resource_monitoring_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Resource Monitoring Example ===");

    let resource_monitor = SystemResourceMonitor::new();

    // Configure monitoring thresholds
    resource_monitor.set_memory_threshold(80.0).await?; // 80% memory usage
    resource_monitor.set_disk_threshold(90.0).await?; // 90% disk usage
    resource_monitor.set_cpu_threshold(85.0).await?; // 85% CPU usage

    println!("Monitoring system resources...");

    // Monitor for 10 seconds
    for i in 0..10 {
        let snapshot = resource_monitor.get_snapshot().await;

        println!("Resource snapshot {}:", i + 1);
        println!(
            "  Memory: {:.1}% ({:.2} GB used / {:.2} GB total)",
            snapshot.memory_usage.percentage,
            snapshot.memory_usage.used_gb,
            snapshot.memory_usage.total_gb
        );

        println!("  CPU: {:.1}%", snapshot.cpu_usage.percentage);

        println!(
            "  Disk: {:.1}% ({:.2} GB free / {:.2} GB total)",
            snapshot.disk_usage.percentage,
            snapshot.disk_usage.free_gb,
            snapshot.disk_usage.total_gb
        );

        // Check for alerts
        if let Some(alerts) = resource_monitor.check_alerts().await {
            for alert in alerts {
                println!("  ⚠️  ALERT: {:?}", alert);
            }
        }

        sleep(Duration::from_secs(1)).await;
    }

    Ok(())
}

/// Example 6: Error handling and recovery strategies
///
/// This example shows comprehensive error handling patterns and
/// recovery strategies for different types of failures.
async fn error_handling_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Error Handling Example ===");

    let factory = create_default_service_factory().await?;
    let processor = factory.video_processor_service().await?;

    // Example URLs with different types of potential failures
    let test_urls = vec![
        "https://nonexistent.example.com/playlist.m3u8", // Network error
        "https://httpbin.org/status/404",                // HTTP 404 error
        "https://httpbin.org/status/500",                // Server error
        "invalid-url",                                   // Invalid URL format
    ];

    for url in test_urls {
        println!("Testing error handling for: {}", url);

        match processor.process_stream(url, "./test_output").await {
            Ok(result) => {
                println!("  ✓ Unexpected success: {:?}", result);
            }
            Err(e) => {
                use video_processor::VideoProcessorError;

                match e.downcast_ref::<VideoProcessorError>() {
                    Some(VideoProcessorError::Network(net_err)) => {
                        println!("  ✗ Network error (will retry): {}", net_err);
                        // In real code, implement retry logic here
                    }
                    Some(VideoProcessorError::Parsing(parse_err)) => {
                        println!("  ✗ Parsing error (check URL format): {}", parse_err);
                        // In real code, try alternative parsing strategies
                    }
                    Some(VideoProcessorError::Configuration(config_err)) => {
                        println!("  ✗ Configuration error (check settings): {}", config_err);
                        // In real code, validate and fix configuration
                    }
                    _ => {
                        println!("  ✗ Other error: {}", e);
                        // In real code, implement generic error handling
                    }
                }
            }
        }
    }

    Ok(())
}

/// Example 7: Configuration management and hot reloading
///
/// This example demonstrates advanced configuration management features
/// including multi-source loading and hot reloading.
async fn configuration_management_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Configuration Management Example ===");

    // Load configuration from multiple sources
    let config_service = video_processor::ConfigService::new();

    // Load base configuration
    let mut config = config_service.load_config().await?;
    println!("Base configuration loaded");

    // Override with environment variables
    config_service
        .apply_environment_overrides(&mut config)
        .await?;
    println!("Environment overrides applied");

    // Validate configuration
    match config_service.validate_config(&config).await {
        Ok(_) => println!("✓ Configuration is valid"),
        Err(e) => {
            println!("✗ Configuration validation failed: {}", e);
            return Err(e.into());
        }
    }

    // Set up configuration watching (in real application)
    println!("Setting up configuration hot reloading...");
    let mut config_changes = config_service.watch_config_changes();

    // Simulate configuration change detection
    tokio::spawn(async move {
        while let Some(new_config) = config_changes.next().await {
            println!("📝 Configuration updated: {:?}", new_config);
            // In real code, apply the new configuration to running services
        }
    });

    println!("Configuration management setup complete");

    Ok(())
}

/// Main function that runs all examples
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎬 Video Processor API Usage Examples\n");

    // Run each example with error handling
    let examples = vec![
        (
            "Basic Video Processing",
            basic_video_processing as fn() -> _,
        ),
        (
            "Advanced Download Management",
            advanced_download_management as fn() -> _,
        ),
        ("M3U8 Parsing", m3u8_parsing_example as fn() -> _),
        (
            "FFmpeg Integration",
            ffmpeg_integration_example as fn() -> _,
        ),
        (
            "Resource Monitoring",
            resource_monitoring_example as fn() -> _,
        ),
        ("Error Handling", error_handling_example as fn() -> _),
        (
            "Configuration Management",
            configuration_management_example as fn() -> _,
        ),
    ];

    for (name, example_fn) in examples {
        println!("\n" + "=".repeat(50).as_str());
        println!("Running: {}", name);
        println!("=".repeat(50));

        match example_fn().await {
            Ok(_) => println!("✅ {} completed successfully", name),
            Err(e) => println!("❌ {} failed: {}", name, e),
        }

        // Small delay between examples
        sleep(Duration::from_millis(500)).await;
    }

    println!("\n🎉 All examples completed!");
    Ok(())
}
