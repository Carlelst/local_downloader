/// 外部系统集成
/// 封装第三方库和外部服务
use anyhow::Result;
use async_trait::async_trait;
use std::process::Stdio;
use tokio::process::{Child, Command};

/// 外部进程管理trait
#[async_trait]
#[async_trait]
pub trait ProcessManager: Send + Sync {
    /// 启动进程
    async fn start_process(&self, command: &str, args: &[&str]) -> Result<ExternalProcess>;

    /// 检查进程状态
    async fn check_process_status(&self, process: &ExternalProcess) -> Result<ProcessStatus>;

    /// 终止进程
    async fn terminate_process(&self, process: &ExternalProcess) -> Result<()>;

    /// 等待进程完成
    async fn wait_for_completion(&self, process: ExternalProcess) -> Result<ProcessResult>;
}

/// 外部进程信息
pub struct ExternalProcess {
    pub id: u32,
    pub child: Child,
    pub command: String,
    pub args: Vec<String>,
    pub start_time: std::time::SystemTime,
}

/// 进程状态
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessStatus {
    Running,
    Completed(i32), // 退出码
    Failed(String),
    NotFound,
}

/// 进程执行结果
#[derive(Debug)]
pub struct ProcessResult {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub execution_time: std::time::Duration,
}

/// 默认进程管理器实现
pub struct DefaultProcessManager;

#[async_trait]
impl ProcessManager for DefaultProcessManager {
    async fn start_process(&self, command: &str, args: &[&str]) -> Result<ExternalProcess> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let child = cmd.spawn()?;
        let id = child.id().unwrap_or(0);

        Ok(ExternalProcess {
            id,
            child,
            command: command.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            start_time: std::time::SystemTime::now(),
        })
    }

    async fn check_process_status(&self, process: &ExternalProcess) -> Result<ProcessStatus> {
        // 简化实现 - 实际应该检查进程是否仍在运行
        #[cfg(unix)]
        {
            use std::process::Command as StdCommand;

            let output = StdCommand::new("ps")
                .arg("-p")
                .arg(process.id.to_string())
                .output();

            match output {
                Ok(output) => {
                    if output.status.success() && !output.stdout.is_empty() {
                        Ok(ProcessStatus::Running)
                    } else {
                        Ok(ProcessStatus::NotFound)
                    }
                }
                Err(_) => Ok(ProcessStatus::NotFound),
            }
        }

        #[cfg(not(unix))]
        {
            // Windows或其他平台的简化实现
            Ok(ProcessStatus::Running)
        }
    }

    async fn terminate_process(&self, process: &ExternalProcess) -> Result<()> {
        #[cfg(unix)]
        {
            use std::process::Command as StdCommand;

            let _ = StdCommand::new("kill")
                .arg("-TERM")
                .arg(process.id.to_string())
                .output();
        }

        #[cfg(windows)]
        {
            use std::process::Command as StdCommand;

            let _ = StdCommand::new("taskkill")
                .arg("/PID")
                .arg(process.id.to_string())
                .arg("/F")
                .output();
        }

        Ok(())
    }

    async fn wait_for_completion(&self, process: ExternalProcess) -> Result<ProcessResult> {
        let start_time = std::time::Instant::now();
        let output = process.child.wait_with_output().await?;
        let execution_time = start_time.elapsed();

        Ok(ProcessResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: output.stderr,
            execution_time,
        })
    }
}

/// FFmpeg进程管理器
pub struct FFmpegManager {
    process_manager: Box<dyn ProcessManager>,
}

impl FFmpegManager {
    pub fn new(process_manager: Box<dyn ProcessManager>) -> Self {
        Self { process_manager }
    }

    pub async fn start_ffmpeg(&self, args: &[&str]) -> Result<ExternalProcess> {
        self.process_manager.start_process("ffmpeg", args).await
    }

    pub async fn check_ffmpeg_availability(&self) -> Result<bool> {
        match self
            .process_manager
            .start_process("ffmpeg", &["-version"])
            .await
        {
            Ok(process) => {
                let result = self.process_manager.wait_for_completion(process).await?;
                Ok(result.exit_code == 0)
            }
            Err(_) => Ok(false),
        }
    }

    pub async fn detect_hardware_encoders(&self) -> Result<Vec<String>> {
        let process = self
            .process_manager
            .start_process("ffmpeg", &["-encoders"])
            .await?;
        let result = self.process_manager.wait_for_completion(process).await?;

        if result.exit_code != 0 {
            return Ok(Vec::new());
        }

        let output = String::from_utf8_lossy(&result.stdout);
        let mut encoders = Vec::new();

        // 解析FFmpeg输出以找到硬件编码器
        for line in output.lines() {
            if line.contains("h264")
                && (line.contains("nvenc") || line.contains("qsv") || line.contains("videotoolbox"))
            {
                if let Some(encoder_name) = line.split_whitespace().nth(1) {
                    encoders.push(encoder_name.to_string());
                }
            }
        }

        Ok(encoders)
    }
}

/// 系统信息收集器
pub struct SystemInfoCollector;

impl SystemInfoCollector {
    pub fn new() -> Self {
        Self
    }

    pub fn get_cpu_count(&self) -> usize {
        num_cpus::get()
    }

    pub fn get_available_memory(&self) -> Result<u64> {
        // 简化实现 - 实际应该使用系统调用
        #[cfg(unix)]
        {
            use std::fs;

            let meminfo = fs::read_to_string("/proc/meminfo")?;
            for line in meminfo.lines() {
                if line.starts_with("MemAvailable:") {
                    if let Some(value_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = value_str.parse::<u64>() {
                            return Ok(kb * 1024); // 转换为字节
                        }
                    }
                }
            }
        }

        // 默认返回值
        Ok(8 * 1024 * 1024 * 1024) // 8GB
    }

    pub fn get_disk_space(&self, path: &std::path::Path) -> Result<(u64, u64)> {
        // 返回 (总空间, 可用空间)
        #[cfg(unix)]
        {
            use std::ffi::CString;
            use std::mem;

            let path_cstr = CString::new(path.to_string_lossy().as_bytes())?;
            let mut statvfs: libc::statvfs = unsafe { mem::zeroed() };

            let result = unsafe { libc::statvfs(path_cstr.as_ptr(), &mut statvfs) };

            if result == 0 {
                let total = (statvfs.f_blocks as u64) * (statvfs.f_frsize as u64);
                let available = (statvfs.f_bavail as u64) * (statvfs.f_frsize as u64);
                Ok((total as u64, available as u64))
            } else {
                Ok((100 * 1024 * 1024 * 1024, 50 * 1024 * 1024 * 1024)) // 默认值
            }
        }

        #[cfg(not(unix))]
        {
            // 非Unix系统的默认值
            Ok((100 * 1024 * 1024 * 1024, 50 * 1024 * 1024 * 1024))
        }
    }
}
