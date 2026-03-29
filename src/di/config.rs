/// 依赖注入配置模块
/// 支持配置驱动的依赖解析
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::lifecycle::Lifecycle;

/// 依赖注入配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DIConfig {
    /// 服务配置映射
    pub services: HashMap<String, ServiceConfig>,

    /// 全局设置
    pub settings: DISettings,
}

/// 服务配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    /// 服务类型名称
    pub service_type: String,

    /// 实现类型名称
    pub implementation_type: String,

    /// 生命周期
    pub lifecycle: LifecycleConfig,

    /// 构造参数
    pub constructor_args: Option<HashMap<String, serde_json::Value>>,

    /// 依赖项
    pub dependencies: Vec<String>,

    /// 是否启用
    pub enabled: bool,
}

/// 生命周期配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleConfig {
    Singleton,
    Transient,
    Scoped,
}

impl From<LifecycleConfig> for Lifecycle {
    fn from(config: LifecycleConfig) -> Self {
        match config {
            LifecycleConfig::Singleton => Lifecycle::Singleton,
            LifecycleConfig::Transient => Lifecycle::Transient,
            LifecycleConfig::Scoped => Lifecycle::Scoped,
        }
    }
}

impl From<Lifecycle> for LifecycleConfig {
    fn from(lifecycle: Lifecycle) -> Self {
        match lifecycle {
            Lifecycle::Singleton => LifecycleConfig::Singleton,
            Lifecycle::Transient => LifecycleConfig::Transient,
            Lifecycle::Scoped => LifecycleConfig::Scoped,
        }
    }
}

/// 依赖注入全局设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DISettings {
    /// 是否启用循环依赖检测
    pub enable_circular_dependency_detection: bool,

    /// 是否启用延迟初始化
    pub enable_lazy_initialization: bool,

    /// 最大依赖深度
    pub max_dependency_depth: usize,

    /// 是否启用性能监控
    pub enable_performance_monitoring: bool,
}

impl Default for DIConfig {
    fn default() -> Self {
        Self {
            services: HashMap::new(),
            settings: DISettings::default(),
        }
    }
}

impl Default for DISettings {
    fn default() -> Self {
        Self {
            enable_circular_dependency_detection: true,
            enable_lazy_initialization: true,
            max_dependency_depth: 10,
            enable_performance_monitoring: false,
        }
    }
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            service_type: String::new(),
            implementation_type: String::new(),
            lifecycle: LifecycleConfig::Transient,
            constructor_args: None,
            dependencies: Vec::new(),
            enabled: true,
        }
    }
}

/// 配置构建器
pub struct DIConfigBuilder {
    config: DIConfig,
}

impl DIConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: DIConfig::default(),
        }
    }

    /// 添加服务配置
    pub fn add_service(mut self, name: String, config: ServiceConfig) -> Self {
        self.config.services.insert(name, config);
        self
    }

    /// 添加单例服务
    pub fn add_singleton<T: 'static>(mut self, name: String, implementation_type: String) -> Self {
        let service_config = ServiceConfig {
            service_type: std::any::type_name::<T>().to_string(),
            implementation_type,
            lifecycle: LifecycleConfig::Singleton,
            constructor_args: None,
            dependencies: Vec::new(),
            enabled: true,
        };

        self.config.services.insert(name, service_config);
        self
    }

    /// 添加瞬态服务
    pub fn add_transient<T: 'static>(mut self, name: String, implementation_type: String) -> Self {
        let service_config = ServiceConfig {
            service_type: std::any::type_name::<T>().to_string(),
            implementation_type,
            lifecycle: LifecycleConfig::Transient,
            constructor_args: None,
            dependencies: Vec::new(),
            enabled: true,
        };

        self.config.services.insert(name, service_config);
        self
    }

    /// 设置全局设置
    pub fn with_settings(mut self, settings: DISettings) -> Self {
        self.config.settings = settings;
        self
    }

    /// 启用循环依赖检测
    pub fn enable_circular_dependency_detection(mut self) -> Self {
        self.config.settings.enable_circular_dependency_detection = true;
        self
    }

    /// 启用性能监控
    pub fn enable_performance_monitoring(mut self) -> Self {
        self.config.settings.enable_performance_monitoring = true;
        self
    }

    /// 构建配置
    pub fn build(self) -> DIConfig {
        self.config
    }
}

impl Default for DIConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 配置加载器
pub struct ConfigLoader;

impl ConfigLoader {
    /// 从JSON文件加载配置
    pub async fn load_from_json(path: &str) -> Result<DIConfig> {
        let content = tokio::fs::read_to_string(path).await?;
        let config: DIConfig = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// 从YAML文件加载配置
    pub async fn load_from_yaml(path: &str) -> Result<DIConfig> {
        let content = tokio::fs::read_to_string(path).await?;
        let config: DIConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// 保存配置到JSON文件
    pub async fn save_to_json(config: &DIConfig, path: &str) -> Result<()> {
        let content = serde_json::to_string_pretty(config)?;
        tokio::fs::write(path, content).await?;
        Ok(())
    }

    /// 保存配置到YAML文件
    pub async fn save_to_yaml(config: &DIConfig, path: &str) -> Result<()> {
        let content = serde_yaml::to_string(config)?;
        tokio::fs::write(path, content).await?;
        Ok(())
    }

    /// 验证配置
    pub fn validate_config(config: &DIConfig) -> Result<()> {
        // 检查服务配置的有效性
        for (name, service_config) in &config.services {
            if service_config.service_type.is_empty() {
                return Err(anyhow::anyhow!("服务 '{}' 的服务类型不能为空", name));
            }

            if service_config.implementation_type.is_empty() {
                return Err(anyhow::anyhow!("服务 '{}' 的实现类型不能为空", name));
            }

            // 检查依赖项是否存在
            for dependency in &service_config.dependencies {
                if !config.services.contains_key(dependency) {
                    return Err(anyhow::anyhow!(
                        "服务 '{}' 的依赖项 '{}' 未找到",
                        name,
                        dependency
                    ));
                }
            }
        }

        // 检查循环依赖
        if config.settings.enable_circular_dependency_detection {
            Self::check_circular_dependencies(config)?;
        }

        Ok(())
    }

    /// 检查循环依赖
    fn check_circular_dependencies(config: &DIConfig) -> Result<()> {
        use std::collections::HashSet;

        fn visit_service(
            service_name: &str,
            config: &DIConfig,
            visiting: &mut HashSet<String>,
            visited: &mut HashSet<String>,
        ) -> Result<()> {
            if visiting.contains(service_name) {
                return Err(anyhow::anyhow!("检测到循环依赖: {}", service_name));
            }

            if visited.contains(service_name) {
                return Ok(());
            }

            visiting.insert(service_name.to_string());

            if let Some(service_config) = config.services.get(service_name) {
                for dependency in &service_config.dependencies {
                    visit_service(dependency, config, visiting, visited)?;
                }
            }

            visiting.remove(service_name);
            visited.insert(service_name.to_string());

            Ok(())
        }

        let mut visited = HashSet::new();

        for service_name in config.services.keys() {
            if !visited.contains(service_name) {
                let mut visiting = HashSet::new();
                visit_service(service_name, config, &mut visiting, &mut visited)?;
            }
        }

        Ok(())
    }
}
