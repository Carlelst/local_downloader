/// 服务注册表
/// 管理服务工厂和元数据
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::any::TypeId;
use std::collections::HashMap;

use super::lifecycle::Lifecycle;

/// 服务工厂trait
#[async_trait]
pub trait ServiceFactory<T>: Send + Sync {
    /// 创建服务实例
    async fn create(&self, container: &crate::di::container::DefaultServiceContainer) -> Result<T>;
}

/// 简单工厂实现 - 用于无依赖的服务
pub struct SimpleFactory<T, F>
where
    F: Fn() -> T + Send + Sync,
{
    factory_fn: F,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, F> SimpleFactory<T, F>
where
    F: Fn() -> T + Send + Sync,
{
    pub fn new(factory_fn: F) -> Self {
        Self {
            factory_fn,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<T, F> ServiceFactory<T> for SimpleFactory<T, F>
where
    T: Send + Sync,
    F: Fn() -> T + Send + Sync,
{
    async fn create(
        &self,
        _container: &crate::di::container::DefaultServiceContainer,
    ) -> Result<T> {
        Ok((self.factory_fn)())
    }
}

/// 异步工厂实现 - 用于需要异步初始化的服务
pub struct AsyncFactory<T, F, Fut>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<T>> + Send,
{
    factory_fn: F,
    _phantom: std::marker::PhantomData<(T, Fut)>,
}

impl<T, F, Fut> AsyncFactory<T, F, Fut>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<T>> + Send,
{
    pub fn new(factory_fn: F) -> Self {
        Self {
            factory_fn,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<T, F, Fut> ServiceFactory<T> for AsyncFactory<T, F, Fut>
where
    T: Send + Sync,
    F: Fn() -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<T>> + Send + Sync,
{
    async fn create(
        &self,
        _container: &crate::di::container::DefaultServiceContainer,
    ) -> Result<T> {
        (self.factory_fn)().await
    }
}

/// 依赖工厂实现 - 用于有依赖的服务
pub struct DependencyFactory<T, F>
where
    F: Fn(&crate::di::container::DefaultServiceContainer) -> Result<T> + Send + Sync,
{
    factory_fn: F,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, F> DependencyFactory<T, F>
where
    F: Fn(&crate::di::container::DefaultServiceContainer) -> Result<T> + Send + Sync,
{
    pub fn new(factory_fn: F) -> Self {
        Self {
            factory_fn,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<T, F> ServiceFactory<T> for DependencyFactory<T, F>
where
    T: Send + Sync,
    F: Fn(&crate::di::container::DefaultServiceContainer) -> Result<T> + Send + Sync,
{
    async fn create(&self, container: &crate::di::container::DefaultServiceContainer) -> Result<T> {
        (self.factory_fn)(container)
    }
}

/// 服务信息
pub struct ServiceInfo<T> {
    pub factory: Box<dyn ServiceFactory<T>>,
    pub lifecycle: Lifecycle,
}

/// 服务注册表
pub struct ServiceRegistry {
    services: HashMap<TypeId, Box<dyn std::any::Any + Send + Sync>>,
}

impl ServiceRegistry {
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
        }
    }

    /// 注册服务
    pub fn register<T: 'static + Send + Sync>(
        &mut self,
        factory: Box<dyn ServiceFactory<T>>,
        lifecycle: Lifecycle,
    ) -> Result<()> {
        let type_id = TypeId::of::<T>();
        let service_info = ServiceInfo { factory, lifecycle };

        self.services.insert(type_id, Box::new(service_info));
        Ok(())
    }

    /// 获取服务信息
    pub fn get_service<T: 'static>(&self) -> Result<&ServiceInfo<T>> {
        let type_id = TypeId::of::<T>();

        self.services
            .get(&type_id)
            .and_then(|service| service.downcast_ref::<ServiceInfo<T>>())
            .ok_or_else(|| anyhow!("服务未注册: {}", std::any::type_name::<T>()))
    }

    /// 检查服务是否已注册
    pub fn is_registered<T: 'static>(&self) -> bool {
        let type_id = TypeId::of::<T>();
        self.services.contains_key(&type_id)
    }

    /// 清除所有服务
    pub fn clear(&mut self) {
        self.services.clear();
    }

    /// 获取已注册服务数量
    pub fn service_count(&self) -> usize {
        self.services.len()
    }

    /// 获取所有已注册的服务类型名称
    pub fn get_registered_types(&self) -> Vec<String> {
        // 简化实现 - 实际应该存储类型名称
        vec!["服务类型信息需要额外存储".to_string()]
    }
}

impl Default for ServiceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 便利宏用于创建工厂
#[macro_export]
macro_rules! simple_factory {
    ($type:ty, $expr:expr) => {
        Box::new($crate::di::registry::SimpleFactory::new(|| $expr))
    };
}

#[macro_export]
macro_rules! async_factory {
    ($type:ty, $expr:expr) => {
        Box::new($crate::di::registry::AsyncFactory::new(|| $expr))
    };
}

#[macro_export]
macro_rules! dependency_factory {
    ($type:ty, $expr:expr) => {
        Box::new($crate::di::registry::DependencyFactory::new($expr))
    };
}
