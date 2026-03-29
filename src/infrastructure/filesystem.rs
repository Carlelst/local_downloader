/// 文件系统基础设施
/// 提供文件操作的具体实现
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// 文件系统操作trait
#[async_trait]
pub trait FileSystem: Send + Sync {
    /// 读取文件内容
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>>;

    /// 写入文件内容
    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()>;

    /// 追加文件内容
    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()>;

    /// 检查文件是否存在
    async fn file_exists(&self, path: &Path) -> bool;

    /// 检查目录是否存在
    async fn dir_exists(&self, path: &Path) -> bool;

    /// 创建目录
    async fn create_dir_all(&self, path: &Path) -> Result<()>;

    /// 删除文件
    async fn remove_file(&self, path: &Path) -> Result<()>;

    /// 删除目录
    async fn remove_dir_all(&self, path: &Path) -> Result<()>;

    /// 获取文件大小
    async fn file_size(&self, path: &Path) -> Result<u64>;

    /// 获取文件元数据
    async fn metadata(&self, path: &Path) -> Result<FileMetadata>;

    /// 列出目录内容
    async fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>>;

    /// 递归列出目录内容
    async fn list_dir_recursive(&self, path: &Path) -> Result<Vec<PathBuf>>;

    /// 获取可用磁盘空间
    async fn available_space(&self, path: &Path) -> Result<u64>;

    /// 复制文件
    async fn copy_file(&self, from: &Path, to: &Path) -> Result<()>;

    /// 移动文件
    async fn move_file(&self, from: &Path, to: &Path) -> Result<()>;

    /// 检查文件权限
    async fn check_permissions(&self, path: &Path, permissions: FilePermissions) -> Result<bool>;

    /// 设置文件权限
    async fn set_permissions(&self, path: &Path, permissions: FilePermissions) -> Result<()>;
}

/// 文件元数据
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub size: u64,
    pub is_file: bool,
    pub is_dir: bool,
    pub created: Option<std::time::SystemTime>,
    pub modified: Option<std::time::SystemTime>,
    pub accessed: Option<std::time::SystemTime>,
    pub permissions: FilePermissions,
}

/// 文件权限
#[derive(Debug, Clone, Copy)]
pub struct FilePermissions {
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
}

/// 文件系统错误类型
#[derive(Debug, thiserror::Error)]
pub enum FileSystemError {
    #[error("文件不存在: {path}")]
    FileNotFound { path: String },

    #[error("目录不存在: {path}")]
    DirectoryNotFound { path: String },

    #[error("权限不足: {path} - {operation}")]
    PermissionDenied { path: String, operation: String },

    #[error("磁盘空间不足: 需要 {required} 字节，可用 {available} 字节")]
    InsufficientSpace { required: u64, available: u64 },

    #[error("文件已存在: {path}")]
    FileExists { path: String },

    #[error("IO错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("路径错误: {0}")]
    InvalidPath(String),
}

/// 默认文件系统实现
pub struct DefaultFileSystem;

impl DefaultFileSystem {
    pub fn new() -> Self {
        Self
    }

    /// 将std::fs::Permissions转换为FilePermissions
    #[cfg(unix)]
    fn convert_permissions(perms: std::fs::Permissions) -> FilePermissions {
        use std::os::unix::fs::PermissionsExt;
        let mode = perms.mode();
        FilePermissions {
            readable: (mode & 0o400) != 0,
            writable: (mode & 0o200) != 0,
            executable: (mode & 0o100) != 0,
        }
    }

    #[cfg(not(unix))]
    fn convert_permissions(perms: std::fs::Permissions) -> FilePermissions {
        FilePermissions {
            readable: !perms.readonly(),
            writable: !perms.readonly(),
            executable: false,
        }
    }
}

#[async_trait]
impl FileSystem for DefaultFileSystem {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        if !self.file_exists(path).await {
            return Err(FileSystemError::FileNotFound {
                path: path.display().to_string(),
            }
            .into());
        }

        let content = fs::read(path)
            .await
            .map_err(|e| anyhow::anyhow!("读取文件失败 {}: {}", path.display(), e))?;
        Ok(content)
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        // 确保父目录存在
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| anyhow::anyhow!("创建父目录失败 {}: {}", parent.display(), e))?;
        }

        fs::write(path, content)
            .await
            .map_err(|e| anyhow::anyhow!("写入文件失败 {}: {}", path.display(), e))?;
        Ok(())
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        use tokio::fs::OpenOptions;

        // 确保父目录存在
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .map_err(|e| anyhow::anyhow!("打开文件失败 {}: {}", path.display(), e))?;

        file.write_all(content)
            .await
            .map_err(|e| anyhow::anyhow!("追加文件失败 {}: {}", path.display(), e))?;
        Ok(())
    }

    async fn file_exists(&self, path: &Path) -> bool {
        tokio::fs::try_exists(path).await.unwrap_or(false) && path.is_file()
    }

    async fn dir_exists(&self, path: &Path) -> bool {
        tokio::fs::try_exists(path).await.unwrap_or(false) && path.is_dir()
    }

    async fn create_dir_all(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path)
            .await
            .map_err(|e| anyhow::anyhow!("创建目录失败 {}: {}", path.display(), e))?;
        Ok(())
    }

    async fn remove_file(&self, path: &Path) -> Result<()> {
        if !self.file_exists(path).await {
            return Err(FileSystemError::FileNotFound {
                path: path.display().to_string(),
            }
            .into());
        }

        fs::remove_file(path)
            .await
            .map_err(|e| anyhow::anyhow!("删除文件失败 {}: {}", path.display(), e))?;
        Ok(())
    }

    async fn remove_dir_all(&self, path: &Path) -> Result<()> {
        if !self.dir_exists(path).await {
            return Err(FileSystemError::DirectoryNotFound {
                path: path.display().to_string(),
            }
            .into());
        }

        fs::remove_dir_all(path)
            .await
            .map_err(|e| anyhow::anyhow!("删除目录失败 {}: {}", path.display(), e))?;
        Ok(())
    }

    async fn file_size(&self, path: &Path) -> Result<u64> {
        let metadata = fs::metadata(path)
            .await
            .map_err(|e| anyhow::anyhow!("获取文件大小失败 {}: {}", path.display(), e))?;
        Ok(metadata.len())
    }

    async fn metadata(&self, path: &Path) -> Result<FileMetadata> {
        let metadata = fs::metadata(path)
            .await
            .map_err(|e| anyhow::anyhow!("获取文件元数据失败 {}: {}", path.display(), e))?;

        Ok(FileMetadata {
            size: metadata.len(),
            is_file: metadata.is_file(),
            is_dir: metadata.is_dir(),
            created: metadata.created().ok(),
            modified: metadata.modified().ok(),
            accessed: metadata.accessed().ok(),
            permissions: Self::convert_permissions(metadata.permissions()),
        })
    }

    async fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>> {
        if !self.dir_exists(path).await {
            return Err(FileSystemError::DirectoryNotFound {
                path: path.display().to_string(),
            }
            .into());
        }

        let mut entries = Vec::new();
        let mut dir = fs::read_dir(path)
            .await
            .map_err(|e| anyhow::anyhow!("读取目录失败 {}: {}", path.display(), e))?;

        while let Some(entry) = dir.next_entry().await? {
            entries.push(entry.path());
        }

        Ok(entries)
    }

    async fn list_dir_recursive(&self, path: &Path) -> Result<Vec<PathBuf>> {
        let mut all_entries = Vec::new();
        let mut stack = vec![path.to_path_buf()];

        while let Some(current_path) = stack.pop() {
            if self.dir_exists(&current_path).await {
                let entries = self.list_dir(&current_path).await?;
                for entry in entries {
                    all_entries.push(entry.clone());
                    if self.dir_exists(&entry).await {
                        stack.push(entry);
                    }
                }
            }
        }

        Ok(all_entries)
    }

    async fn available_space(&self, path: &Path) -> Result<u64> {
        #[cfg(unix)]
        {
            use std::ffi::CString;
            use std::mem;

            let path_str = path.to_string_lossy();
            let path_cstr = CString::new(path_str.as_bytes())
                .map_err(|e| anyhow::anyhow!("路径转换失败: {}", e))?;
            let mut statvfs: libc::statvfs = unsafe { mem::zeroed() };

            let result = unsafe { libc::statvfs(path_cstr.as_ptr(), &mut statvfs) };

            if result == 0 {
                let available = (statvfs.f_bavail as u64) * (statvfs.f_frsize as u64);
                Ok(available)
            } else {
                Err(anyhow::anyhow!("获取磁盘空间失败"))
            }
        }

        #[cfg(not(unix))]
        {
            // 非Unix系统的简化实现
            Ok(1024 * 1024 * 1024) // 1GB
        }
    }

    async fn copy_file(&self, from: &Path, to: &Path) -> Result<()> {
        if !self.file_exists(from).await {
            return Err(FileSystemError::FileNotFound {
                path: from.display().to_string(),
            }
            .into());
        }

        // 确保目标目录存在
        if let Some(parent) = to.parent() {
            self.create_dir_all(parent).await?;
        }

        fs::copy(from, to).await.map_err(|e| {
            anyhow::anyhow!("复制文件失败 {} -> {}: {}", from.display(), to.display(), e)
        })?;
        Ok(())
    }

    async fn move_file(&self, from: &Path, to: &Path) -> Result<()> {
        if !self.file_exists(from).await {
            return Err(FileSystemError::FileNotFound {
                path: from.display().to_string(),
            }
            .into());
        }

        // 确保目标目录存在
        if let Some(parent) = to.parent() {
            self.create_dir_all(parent).await?;
        }

        fs::rename(from, to).await.map_err(|e| {
            anyhow::anyhow!("移动文件失败 {} -> {}: {}", from.display(), to.display(), e)
        })?;
        Ok(())
    }

    async fn check_permissions(&self, path: &Path, permissions: FilePermissions) -> Result<bool> {
        let metadata = self.metadata(path).await?;

        let has_read = !permissions.readable || metadata.permissions.readable;
        let has_write = !permissions.writable || metadata.permissions.writable;
        let has_exec = !permissions.executable || metadata.permissions.executable;

        Ok(has_read && has_write && has_exec)
    }

    async fn set_permissions(&self, path: &Path, permissions: FilePermissions) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut mode = 0o000;
            if permissions.readable {
                mode |= 0o400;
            }
            if permissions.writable {
                mode |= 0o200;
            }
            if permissions.executable {
                mode |= 0o100;
            }

            let perms = std::fs::Permissions::from_mode(mode);
            fs::set_permissions(path, perms)
                .await
                .map_err(|e| anyhow::anyhow!("设置文件权限失败 {}: {}", path.display(), e))?;
        }

        #[cfg(not(unix))]
        {
            let perms = std::fs::Permissions::from_readonly(!permissions.writable);
            fs::set_permissions(path, perms)
                .await
                .map_err(|e| anyhow::anyhow!("设置文件权限失败 {}: {}", path.display(), e))?;
        }

        Ok(())
    }
}

impl Default for DefaultFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for FilePermissions {
    fn default() -> Self {
        Self {
            readable: true,
            writable: true,
            executable: false,
        }
    }
}

/// 临时文件管理器
pub struct TempFileManager {
    temp_dir: PathBuf,
    filesystem: Box<dyn FileSystem>,
}

impl TempFileManager {
    pub fn new(temp_dir: PathBuf, filesystem: Box<dyn FileSystem>) -> Self {
        Self {
            temp_dir,
            filesystem,
        }
    }

    pub async fn create_temp_file(&self, prefix: &str, suffix: &str) -> Result<PathBuf> {
        self.filesystem.create_dir_all(&self.temp_dir).await?;

        let filename = format!("{}_{}{}", prefix, uuid::Uuid::new_v4(), suffix);
        let path = self.temp_dir.join(filename);

        Ok(path)
    }

    pub async fn cleanup_temp_files(&self, max_age: std::time::Duration) -> Result<usize> {
        let entries = self.filesystem.list_dir(&self.temp_dir).await?;
        let mut cleaned = 0;

        for entry in entries {
            if let Ok(metadata) = std::fs::metadata(&entry) {
                if let Ok(created) = metadata.created() {
                    if created.elapsed().unwrap_or_default() > max_age {
                        if self.filesystem.remove_file(&entry).await.is_ok() {
                            cleaned += 1;
                        }
                    }
                }
            }
        }

        Ok(cleaned)
    }
}
