# Video Processor API Documentation

## Overview

The Video Processor is a high-performance Rust application for downloading and processing M3U8 video streams. It features concurrent downloading, AES decryption, hardware-accelerated encoding, and comprehensive error handling.

## Architecture

The application follows a layered architecture pattern:

- **Presentation Layer**: CLI interfaces and configuration management
- **Application Layer**: Business logic coordination and workflow management  
- **Domain Layer**: Core business logic and entities
- **Infrastructure Layer**: External service integration and data persistence

## Quick Start

### Basic Usage

```rust
use video_processor::{load_config, VideoProcessorService, create_default_service_factory};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = load_config()?;
    
    // Create service factory
    let factory = create_default_service_factory().await?;
    
    // Get video processor service
    let processor = factory.video_processor_service().await?;
    
    // Process M3U8 URLs
    for url in &config.m3u8_urls {
        let result = processor.process_stream(url, &config.output_directory).await?;
        println!("Processed: {:?}", result);
    }
    
    Ok(())
}
```

### Configuration

Create a `config.yaml` file:

```yaml
m3u8_urls:
  - "https://example.com/playlist.m3u8"
max_concurrent_downloads: 10
download_retries: 3
segment_timeout: 30
output_directory: "./downloads"
log_level: "info"
video_format: "mp4"
use_hw_accel: true
```

## Core Components

### Video Processor Service

The main service for processing video streams:

```rust
use video_processor::VideoProcessorService;

// Process a single stream
let result = processor.process_stream(url, output_dir).await?;

// Process multiple streams concurrently
let results = processor.process_multiple_streams(urls, output_dir).await?;

// Monitor progress
let progress = processor.get_progress().await?;
```

### Download Manager

Handles concurrent downloading with adaptive strategies:

```rust
use video_processor::AdaptiveDownloadManager;

let download_manager = AdaptiveDownloadManager::new(config);

// Download segments with automatic retry
let segments = download_manager.download_segments(urls).await?;

// Get download statistics
let stats = download_manager.get_download_stats();
```

### M3U8 Parser

Parses M3U8 playlist files:

```rust
use video_processor::DefaultM3U8Parser;

let parser = DefaultM3U8Parser::new();

// Parse playlist from URL
let playlist = parser.parse(url).await?;

// Resolve segment URLs
let segments = parser.resolve_segments(&playlist).await?;
```

### FFmpeg Handler

Manages FFmpeg processes for video encoding:

```rust
use video_processor::DefaultFFmpegHandler;

let ffmpeg = DefaultFFmpegHandler::new();

// Detect hardware acceleration
let accelerators = ffmpeg.detect_hardware_acceleration().await;

// Start encoding process
let process = ffmpeg.start_process(&config).await?;
```

## Error Handling

The library uses a comprehensive error handling system:

```rust
use video_processor::VideoProcessorError;

match result {
    Ok(data) => println!("Success: {:?}", data),
    Err(VideoProcessorError::Network(e)) => {
        // Handle network errors with retry
        eprintln!("Network error: {}", e);
    },
    Err(VideoProcessorError::Parsing(e)) => {
        // Handle parsing errors
        eprintln!("Parsing error: {}", e);
    },
    Err(e) => eprintln!("Other error: {}", e),
}
```

## Monitoring and Logging

### Progress Monitoring

```rust
use video_processor::DefaultProgressMonitor;

let monitor = DefaultProgressMonitor::new();

// Get current progress
let progress = monitor.get_progress().await;
println!("Progress: {:.2}%", progress.percentage);

// Get detailed statistics
let stats = monitor.get_statistics().await;
```

### Structured Logging

```rust
use video_processor::{init_console_logger, LogLevel};

// Initialize logging
init_console_logger(LogLevel::Info)?;

// Logs will include structured context information
log::info!("Processing started for URL: {}", url);
```

## Advanced Features

### Dependency Injection

The application uses a dependency injection container:

```rust
use video_processor::di::{ServiceContainer, ServiceRegistry};

let mut container = ServiceContainer::new();
container.register_singleton::<VideoProcessorService>()?;

let service = container.resolve::<VideoProcessorService>().await?;
```

### Configuration Management

Multi-source configuration with hot reloading:

```rust
use video_processor::ConfigService;

let config_service = ConfigService::new();

// Load from multiple sources
let config = config_service.load_config().await?;

// Watch for changes
let mut changes = config_service.watch_config_changes();
while let Some(new_config) = changes.next().await {
    println!("Configuration updated: {:?}", new_config);
}
```

### Resource Monitoring

Monitor system resources and automatically adjust behavior:

```rust
use video_processor::SystemResourceMonitor;

let monitor = SystemResourceMonitor::new();

// Get current resource usage
let snapshot = monitor.get_snapshot().await;
println!("Memory usage: {:.2}%", snapshot.memory_usage.percentage);

// Set up alerts
monitor.set_memory_threshold(80.0).await?;
```

## Best Practices

### 1. Configuration Management

- Use environment-specific configuration files
- Leverage environment variables for sensitive data
- Implement configuration validation

### 2. Error Handling

- Always handle network errors with appropriate retry strategies
- Use structured error types for better debugging
- Implement graceful degradation for non-critical failures

### 3. Performance Optimization

- Configure concurrent downloads based on available bandwidth
- Use hardware acceleration when available
- Monitor resource usage and adjust accordingly

### 4. Monitoring

- Enable structured logging for production deployments
- Set up resource monitoring and alerts
- Track download statistics and performance metrics

## Examples

See the `examples/` directory for complete working examples:

- `enhanced_main.rs`: Full-featured application with all optimizations
- `logging_example.rs`: Demonstrates structured logging setup
- `new_architecture_example.rs`: Shows new architecture usage

## API Reference

For detailed API documentation, run:

```bash
cargo doc --open
```

This will generate and open the complete API documentation in your browser.