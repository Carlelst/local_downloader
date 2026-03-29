/// 服务生命周期管理
/// 定义服务实例的创建和销毁策略

/// 服务生命周期枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// 单例 - 整个应用程序生命周期内只创建一个实例
    Singleton,

    /// 瞬态 - 每次请求都创建新实例
    Transient,

    /// 作用域 - 在特定作用域内共享实例
    Scoped,
}

impl Lifecycle {
    /// 获取生命周期描述
    pub fn description(&self) -> &'static str {
        match self {
            Lifecycle::Singleton => "单例 - 应用程序生命周期内共享一个实例",
            Lifecycle::Transient => "瞬态 - 每次请求创建新实例",
            Lifecycle::Scoped => "作用域 - 在特定作用域内共享实例",
        }
    }

    /// 检查是否需要缓存实例
    pub fn should_cache(&self) -> bool {
        matches!(self, Lifecycle::Singleton | Lifecycle::Scoped)
    }

    /// 检查是否为单例
    pub fn is_singleton(&self) -> bool {
        matches!(self, Lifecycle::Singleton)
    }

    /// 检查是否为瞬态
    pub fn is_transient(&self) -> bool {
        matches!(self, Lifecycle::Transient)
    }

    /// 检查是否为作用域
    pub fn is_scoped(&self) -> bool {
        matches!(self, Lifecycle::Scoped)
    }
}

impl Default for Lifecycle {
    fn default() -> Self {
        Lifecycle::Transient
    }
}

impl std::fmt::Display for Lifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Lifecycle::Singleton => "Singleton",
            Lifecycle::Transient => "Transient",
            Lifecycle::Scoped => "Scoped",
        };
        write!(f, "{}", name)
    }
}

/// 作用域管理器
/// 用于管理作用域生命周期的服务实例
#[derive(Clone)]
pub struct ScopeManager {
    scope_id: String,
    instances: std::collections::HashMap<
        std::any::TypeId,
        std::sync::Arc<dyn std::any::Any + Send + Sync>,
    >,
}

impl ScopeManager {
    pub fn new(scope_id: String) -> Self {
        Self {
            scope_id,
            instances: std::collections::HashMap::new(),
        }
    }

    pub fn scope_id(&self) -> &str {
        &self.scope_id
    }

    /// 获取作用域内的实例
    pub fn get_instance<T: 'static + Send + Sync>(&self) -> Option<std::sync::Arc<T>> {
        let type_id = std::any::TypeId::of::<T>();
        self.instances
            .get(&type_id)
            .and_then(|instance| instance.clone().downcast::<T>().ok())
    }

    /// 设置作用域内的实例
    pub fn set_instance<T: 'static + Send + Sync>(&mut self, instance: std::sync::Arc<T>) {
        let type_id = std::any::TypeId::of::<T>();
        self.instances.insert(
            type_id,
            instance as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );
    }

    /// 清除所有实例
    pub fn clear(&mut self) {
        self.instances.clear();
    }

    /// 获取实例数量
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }
}

/// 作用域上下文
/// 提供作用域生命周期的上下文管理
pub struct ScopeContext {
    scopes: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, ScopeManager>>>,
}

impl ScopeContext {
    pub fn new() -> Self {
        Self {
            scopes: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// 创建新作用域
    pub async fn create_scope(&self, scope_id: String) -> anyhow::Result<()> {
        let mut scopes = self.scopes.write().await;

        if scopes.contains_key(&scope_id) {
            return Err(anyhow::anyhow!("作用域已存在: {}", scope_id));
        }

        scopes.insert(scope_id.clone(), ScopeManager::new(scope_id));
        Ok(())
    }

    /// 获取作用域管理器
    pub async fn get_scope(&self, scope_id: &str) -> Option<ScopeManager> {
        let scopes = self.scopes.read().await;
        scopes.get(scope_id).cloned()
    }

    /// 销毁作用域
    pub async fn destroy_scope(&self, scope_id: &str) -> anyhow::Result<()> {
        let mut scopes = self.scopes.write().await;

        if scopes.remove(scope_id).is_some() {
            log::debug!("作用域已销毁: {}", scope_id);
            Ok(())
        } else {
            Err(anyhow::anyhow!("作用域不存在: {}", scope_id))
        }
    }

    /// 清除所有作用域
    pub async fn clear_all_scopes(&self) {
        let mut scopes = self.scopes.write().await;
        scopes.clear();
        log::debug!("所有作用域已清除");
    }

    /// 获取活跃作用域数量
    pub async fn active_scope_count(&self) -> usize {
        let scopes = self.scopes.read().await;
        scopes.len()
    }

    /// 获取所有作用域ID
    pub async fn get_scope_ids(&self) -> Vec<String> {
        let scopes = self.scopes.read().await;
        scopes.keys().cloned().collect()
    }
}

impl Default for ScopeContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ScopeContext {
    fn clone(&self) -> Self {
        Self {
            scopes: self.scopes.clone(),
        }
    }
}
