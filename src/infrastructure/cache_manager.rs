/// 缓存管理器基础设施实现
/// 提供HTTP连接池复用和基础缓存策略
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::time::interval;

/// 缓存配置
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// 最大缓存条目数
    pub max_entries: usize,
    /// 缓存过期时间
    pub ttl: Duration,
    /// 清理间隔
    pub cleanup_interval: Duration,
    /// 最大内存使用量（字节）
    pub max_memory_usage: usize,
    /// LRU淘汰阈值
    pub eviction_threshold: f64,
}

/// 缓存条目
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    value: T,
    created_at: SystemTime,
    last_accessed: SystemTime,
    access_count: u64,
    size: usize,
}

/// LRU缓存实现
pub struct LRUCache<K, V>
where
    K: Clone + Eq + std::hash::Hash,
    V: Clone,
{
    entries: Arc<RwLock<HashMap<K, CacheEntry<V>>>>,
    access_order: Arc<RwLock<Vec<K>>>,
    config: CacheConfig,
    current_memory_usage: Arc<RwLock<usize>>,
    stats: Arc<RwLock<CacheStats>>,
}

/// 缓存统计信息
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub total_entries: usize,
    pub memory_usage: usize,
    pub hit_rate: f64,
}

/// HTTP连接池管理器
pub struct HttpConnectionPool {
    pools: Arc<RwLock<HashMap<String, ConnectionPool>>>,
    config: ConnectionPoolConfig,
}

/// 连接池配置
#[derive(Debug, Clone)]
pub struct ConnectionPoolConfig {
    /// 每个主机的最大连接数
    pub max_connections_per_host: usize,
    /// 连接超时时间
    pub connection_timeout: Duration,
    /// 空闲连接超时时间
    pub idle_timeout: Duration,
    /// 连接保活时间
    pub keep_alive_timeout: Duration,
    /// 最大空闲连接数
    pub max_idle_connections: usize,
}

/// 单个主机的连接池
struct ConnectionPool {
    active_connections: usize,
    idle_connections: Vec<Connection>,
    last_used: SystemTime,
    stats: ConnectionPoolStats,
}

/// 连接对象
#[derive(Debug, Clone)]
struct Connection {
    id: String,
    created_at: SystemTime,
    last_used: SystemTime,
    request_count: u64,
    is_active: bool,
}

/// 连接池统计信息
#[derive(Debug, Clone, Default, serde::Serialize)]
struct ConnectionPoolStats {
    total_connections_created: u64,
    total_connections_reused: u64,
    total_connections_closed: u64,
    current_active_connections: usize,
    current_idle_connections: usize,
}

/// 缓存策略枚举
#[derive(Debug, Clone, Copy)]
pub enum CacheStrategy {
    LRU,      // 最近最少使用
    LFU,      // 最少使用频率
    FIFO,     // 先进先出
    TTL,      // 基于时间过期
    Adaptive, // 自适应策略
}

/// 自适应缓存管理器
pub struct AdaptiveCacheManager {
    http_cache: Arc<LRUCache<String, Vec<u8>>>,
    metadata_cache: Arc<LRUCache<String, serde_json::Value>>,
    connection_pool: Arc<HttpConnectionPool>,
    strategy: CacheStrategy,
    performance_monitor: Arc<RwLock<CachePerformanceMonitor>>,
}

/// 缓存性能监控器
#[derive(Debug, Default)]
struct CachePerformanceMonitor {
    response_times: Vec<Duration>,
    memory_usage_history: Vec<(SystemTime, usize)>,
    hit_rate_history: Vec<(SystemTime, f64)>,
    strategy_performance: HashMap<CacheStrategy, StrategyPerformance>,
}

/// 策略性能指标
#[derive(Debug, Default, Clone)]
struct StrategyPerformance {
    average_response_time: Duration,
    hit_rate: f64,
    memory_efficiency: f64,
    eviction_rate: f64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 1000,
            ttl: Duration::from_secs(3600),             // 1小时
            cleanup_interval: Duration::from_secs(300), // 5分钟
            max_memory_usage: 100 * 1024 * 1024,        // 100MB
            eviction_threshold: 0.8,
        }
    }
}

impl Default for ConnectionPoolConfig {
    fn default() -> Self {
        Self {
            max_connections_per_host: 10,
            connection_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(90),
            keep_alive_timeout: Duration::from_secs(60),
            max_idle_connections: 5,
        }
    }
}

impl<K, V> LRUCache<K, V>
where
    K: Clone + Eq + std::hash::Hash + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    pub fn new(config: CacheConfig) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            access_order: Arc::new(RwLock::new(Vec::new())),
            config,
            current_memory_usage: Arc::new(RwLock::new(0)),
            stats: Arc::new(RwLock::new(CacheStats::default())),
        }
    }

    /// 获取缓存值
    pub async fn get(&self, key: &K) -> Option<V> {
        let mut entries = self.entries.write().await;
        let mut access_order = self.access_order.write().await;
        let mut stats = self.stats.write().await;

        if let Some(entry) = entries.get_mut(key) {
            // 更新访问信息
            entry.last_accessed = SystemTime::now();
            entry.access_count += 1;

            // 更新LRU顺序
            if let Some(pos) = access_order.iter().position(|k| k == key) {
                let key_clone = access_order.remove(pos);
                access_order.push(key_clone);
            }

            // 更新统计信息
            stats.hits += 1;
            stats.hit_rate = stats.hits as f64 / (stats.hits + stats.misses) as f64;

            Some(entry.value.clone())
        } else {
            // 缓存未命中
            stats.misses += 1;
            stats.hit_rate = stats.hits as f64 / (stats.hits + stats.misses) as f64;
            None
        }
    }

    /// 设置缓存值
    pub async fn set(&self, key: K, value: V, size: usize) -> Result<()> {
        let mut entries = self.entries.write().await;
        let mut access_order = self.access_order.write().await;
        let mut current_memory = self.current_memory_usage.write().await;
        let mut stats = self.stats.write().await;

        // 检查是否需要淘汰
        while self.should_evict(&entries, *current_memory + size).await {
            if let Some(lru_key) = access_order.first().cloned() {
                if let Some(old_entry) = entries.remove(&lru_key) {
                    *current_memory -= old_entry.size;
                    stats.evictions += 1;
                }
                access_order.remove(0);
            } else {
                break;
            }
        }

        // 创建新条目
        let entry = CacheEntry {
            value,
            created_at: SystemTime::now(),
            last_accessed: SystemTime::now(),
            access_count: 1,
            size,
        };

        // 如果键已存在，更新内存使用量
        if let Some(old_entry) = entries.get(&key) {
            *current_memory -= old_entry.size;
        } else {
            // 新键，添加到访问顺序
            access_order.push(key.clone());
        }

        entries.insert(key, entry);
        *current_memory += size;

        // 更新统计信息
        stats.total_entries = entries.len();
        stats.memory_usage = *current_memory;

        Ok(())
    }

    /// 删除缓存条目
    pub async fn remove(&self, key: &K) -> Option<V> {
        let mut entries = self.entries.write().await;
        let mut access_order = self.access_order.write().await;
        let mut current_memory = self.current_memory_usage.write().await;
        let mut stats = self.stats.write().await;

        if let Some(entry) = entries.remove(key) {
            *current_memory -= entry.size;

            // 从访问顺序中移除
            if let Some(pos) = access_order.iter().position(|k| k == key) {
                access_order.remove(pos);
            }

            // 更新统计信息
            stats.total_entries = entries.len();
            stats.memory_usage = *current_memory;

            Some(entry.value)
        } else {
            None
        }
    }

    /// 清空缓存
    pub async fn clear(&self) {
        let mut entries = self.entries.write().await;
        let mut access_order = self.access_order.write().await;
        let mut current_memory = self.current_memory_usage.write().await;
        let mut stats = self.stats.write().await;

        entries.clear();
        access_order.clear();
        *current_memory = 0;

        stats.total_entries = 0;
        stats.memory_usage = 0;
    }

    /// 获取缓存统计信息
    pub async fn get_stats(&self) -> CacheStats {
        self.stats.read().await.clone()
    }

    /// 检查是否需要淘汰
    async fn should_evict(
        &self,
        entries: &HashMap<K, CacheEntry<V>>,
        projected_memory: usize,
    ) -> bool {
        entries.len() >= self.config.max_entries
            || projected_memory > self.config.max_memory_usage
            || (projected_memory as f64 / self.config.max_memory_usage as f64)
                > self.config.eviction_threshold
    }

    /// 启动定期清理
    pub async fn start_cleanup(&self) -> Result<()> {
        let entries = Arc::clone(&self.entries);
        let access_order = Arc::clone(&self.access_order);
        let current_memory = Arc::clone(&self.current_memory_usage);
        let stats = Arc::clone(&self.stats);
        let ttl = self.config.ttl;
        let cleanup_interval = self.config.cleanup_interval;

        tokio::spawn(async move {
            let mut interval = interval(cleanup_interval);

            loop {
                interval.tick().await;

                let mut entries_guard = entries.write().await;
                let mut access_order_guard = access_order.write().await;
                let mut memory_guard = current_memory.write().await;
                let mut stats_guard = stats.write().await;

                let now = SystemTime::now();
                let mut expired_keys = Vec::new();

                // 找出过期的条目
                for (key, entry) in entries_guard.iter() {
                    if let Ok(elapsed) = now.duration_since(entry.created_at) {
                        if elapsed > ttl {
                            expired_keys.push(key.clone());
                        }
                    }
                }

                // 删除过期条目
                for key in expired_keys {
                    if let Some(entry) = entries_guard.remove(&key) {
                        *memory_guard -= entry.size;
                        stats_guard.evictions += 1;

                        // 从访问顺序中移除
                        if let Some(pos) = access_order_guard.iter().position(|k| k == &key) {
                            access_order_guard.remove(pos);
                        }
                    }
                }

                // 更新统计信息
                stats_guard.total_entries = entries_guard.len();
                stats_guard.memory_usage = *memory_guard;

                log::debug!(
                    "缓存清理完成，当前条目数: {}, 内存使用: {} 字节",
                    stats_guard.total_entries,
                    stats_guard.memory_usage
                );
            }
        });

        Ok(())
    }
}

impl HttpConnectionPool {
    pub fn new(config: ConnectionPoolConfig) -> Self {
        Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// 获取连接
    pub async fn get_connection(&self, host: &str) -> Result<Connection> {
        let mut pools = self.pools.write().await;
        let pool = pools
            .entry(host.to_string())
            .or_insert_with(|| ConnectionPool {
                active_connections: 0,
                idle_connections: Vec::new(),
                last_used: SystemTime::now(),
                stats: ConnectionPoolStats::default(),
            });

        // 尝试复用空闲连接
        if let Some(mut connection) = pool.idle_connections.pop() {
            connection.last_used = SystemTime::now();
            connection.request_count += 1;
            connection.is_active = true;

            pool.active_connections += 1;
            pool.stats.total_connections_reused += 1;
            pool.stats.current_active_connections = pool.active_connections;
            pool.stats.current_idle_connections = pool.idle_connections.len();

            log::debug!("复用连接到 {}, 连接ID: {}", host, connection.id);
            return Ok(connection);
        }

        // 检查是否可以创建新连接
        if pool.active_connections >= self.config.max_connections_per_host {
            return Err(anyhow::anyhow!("达到主机 {} 的最大连接数限制", host));
        }

        // 创建新连接
        let connection = Connection {
            id: format!(
                "conn_{}_{}",
                host,
                SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis()
            ),
            created_at: SystemTime::now(),
            last_used: SystemTime::now(),
            request_count: 1,
            is_active: true,
        };

        pool.active_connections += 1;
        pool.stats.total_connections_created += 1;
        pool.stats.current_active_connections = pool.active_connections;

        log::debug!("创建新连接到 {}, 连接ID: {}", host, connection.id);
        Ok(connection)
    }

    /// 归还连接
    pub async fn return_connection(&self, host: &str, mut connection: Connection) -> Result<()> {
        let mut pools = self.pools.write().await;

        if let Some(pool) = pools.get_mut(host) {
            connection.is_active = false;
            pool.active_connections = pool.active_connections.saturating_sub(1);

            // 检查连接是否应该保持
            if pool.idle_connections.len() < self.config.max_idle_connections {
                pool.idle_connections.push(connection);
                pool.stats.current_idle_connections = pool.idle_connections.len();
                log::debug!("连接归还到空闲池: {}", host);
            } else {
                pool.stats.total_connections_closed += 1;
                log::debug!("连接已关闭（空闲池已满）: {}", host);
            }

            pool.stats.current_active_connections = pool.active_connections;
            pool.last_used = SystemTime::now();
        }

        Ok(())
    }

    /// 清理过期连接
    pub async fn cleanup_expired_connections(&self) -> Result<()> {
        let mut pools = self.pools.write().await;
        let now = SystemTime::now();

        for (host, pool) in pools.iter_mut() {
            // 清理空闲连接
            pool.idle_connections.retain(|conn| {
                if let Ok(elapsed) = now.duration_since(conn.last_used) {
                    if elapsed > self.config.idle_timeout {
                        pool.stats.total_connections_closed += 1;
                        log::debug!("清理过期空闲连接: {} -> {}", host, conn.id);
                        false
                    } else {
                        true
                    }
                } else {
                    true
                }
            });

            pool.stats.current_idle_connections = pool.idle_connections.len();
        }

        // 移除长时间未使用的连接池
        pools.retain(|host, pool| {
            if let Ok(elapsed) = now.duration_since(pool.last_used) {
                if elapsed > Duration::from_secs(3600)
                    && pool.active_connections == 0
                    && pool.idle_connections.is_empty()
                {
                    log::debug!("移除未使用的连接池: {}", host);
                    false
                } else {
                    true
                }
            } else {
                true
            }
        });

        Ok(())
    }

    /// 获取连接池统计信息
    pub async fn get_stats(&self) -> HashMap<String, ConnectionPoolStats> {
        let pools = self.pools.read().await;
        pools
            .iter()
            .map(|(host, pool)| (host.clone(), pool.stats.clone()))
            .collect()
    }

    /// 启动定期清理
    pub async fn start_cleanup(&self) -> Result<()> {
        let pool = Arc::new(self.clone());

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(60)); // 每分钟清理一次

            loop {
                interval.tick().await;

                if let Err(e) = pool.cleanup_expired_connections().await {
                    log::error!("连接池清理失败: {}", e);
                }
            }
        });

        Ok(())
    }
}

impl Clone for HttpConnectionPool {
    fn clone(&self) -> Self {
        Self {
            pools: Arc::clone(&self.pools),
            config: self.config.clone(),
        }
    }
}

impl AdaptiveCacheManager {
    pub fn new() -> Self {
        let cache_config = CacheConfig::default();
        let pool_config = ConnectionPoolConfig::default();

        Self {
            http_cache: Arc::new(LRUCache::new(cache_config.clone())),
            metadata_cache: Arc::new(LRUCache::new(cache_config)),
            connection_pool: Arc::new(HttpConnectionPool::new(pool_config)),
            strategy: CacheStrategy::Adaptive,
            performance_monitor: Arc::new(RwLock::new(CachePerformanceMonitor::default())),
        }
    }

    /// 缓存HTTP响应
    pub async fn cache_http_response(&self, url: &str, data: Vec<u8>) -> Result<()> {
        let size = data.len();
        self.http_cache.set(url.to_string(), data, size).await
    }

    /// 获取缓存的HTTP响应
    pub async fn get_cached_http_response(&self, url: &str) -> Option<Vec<u8>> {
        self.http_cache.get(&url.to_string()).await
    }

    /// 缓存元数据
    pub async fn cache_metadata(&self, key: &str, metadata: serde_json::Value) -> Result<()> {
        let size = metadata.to_string().len();
        self.metadata_cache
            .set(key.to_string(), metadata, size)
            .await
    }

    /// 获取缓存的元数据
    pub async fn get_cached_metadata(&self, key: &str) -> Option<serde_json::Value> {
        self.metadata_cache.get(&key.to_string()).await
    }

    /// 获取HTTP连接
    pub async fn get_http_connection(&self, host: &str) -> Result<Connection> {
        self.connection_pool.get_connection(host).await
    }

    /// 归还HTTP连接
    pub async fn return_http_connection(&self, host: &str, connection: Connection) -> Result<()> {
        self.connection_pool
            .return_connection(host, connection)
            .await
    }

    /// 启动所有清理任务
    pub async fn start_all_cleanup_tasks(&self) -> Result<()> {
        self.http_cache.start_cleanup().await?;
        self.metadata_cache.start_cleanup().await?;
        self.connection_pool.start_cleanup().await?;

        log::info!("所有缓存清理任务已启动");
        Ok(())
    }

    /// 获取综合统计信息
    pub async fn get_comprehensive_stats(&self) -> Result<serde_json::Value> {
        let http_stats = self.http_cache.get_stats().await;
        let metadata_stats = self.metadata_cache.get_stats().await;
        let connection_stats = self.connection_pool.get_stats().await;

        let stats = serde_json::json!({
            "http_cache": {
                "hits": http_stats.hits,
                "misses": http_stats.misses,
                "hit_rate": http_stats.hit_rate,
                "total_entries": http_stats.total_entries,
                "memory_usage": http_stats.memory_usage,
                "evictions": http_stats.evictions
            },
            "metadata_cache": {
                "hits": metadata_stats.hits,
                "misses": metadata_stats.misses,
                "hit_rate": metadata_stats.hit_rate,
                "total_entries": metadata_stats.total_entries,
                "memory_usage": metadata_stats.memory_usage,
                "evictions": metadata_stats.evictions
            },
            "connection_pools": connection_stats,
            "strategy": format!("{:?}", self.strategy)
        });

        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lru_cache_basic_operations() {
        let cache = LRUCache::new(CacheConfig::default());

        // 测试设置和获取
        cache
            .set("key1".to_string(), "value1".to_string(), 10)
            .await
            .unwrap();
        let value = cache.get(&"key1".to_string()).await;
        assert_eq!(value, Some("value1".to_string()));

        // 测试缓存未命中
        let missing = cache.get(&"missing".to_string()).await;
        assert_eq!(missing, None);

        // 测试删除
        let removed = cache.remove(&"key1".to_string()).await;
        assert_eq!(removed, Some("value1".to_string()));

        let after_remove = cache.get(&"key1".to_string()).await;
        assert_eq!(after_remove, None);
    }

    #[tokio::test]
    async fn test_lru_cache_eviction() {
        let config = CacheConfig {
            max_entries: 2,
            ..CacheConfig::default()
        };
        let cache = LRUCache::new(config);

        // 添加两个条目
        cache
            .set("key1".to_string(), "value1".to_string(), 10)
            .await
            .unwrap();
        cache
            .set("key2".to_string(), "value2".to_string(), 10)
            .await
            .unwrap();

        // 访问第一个条目使其成为最近使用
        let _ = cache.get(&"key1".to_string()).await;

        // 添加第三个条目，应该淘汰key2
        cache
            .set("key3".to_string(), "value3".to_string(), 10)
            .await
            .unwrap();

        // key2应该被淘汰
        assert_eq!(cache.get(&"key2".to_string()).await, None);
        // key1和key3应该还在
        assert_eq!(
            cache.get(&"key1".to_string()).await,
            Some("value1".to_string())
        );
        assert_eq!(
            cache.get(&"key3".to_string()).await,
            Some("value3".to_string())
        );
    }

    #[tokio::test]
    async fn test_connection_pool() {
        let pool = HttpConnectionPool::new(ConnectionPoolConfig::default());

        // 获取连接
        let conn1 = pool.get_connection("example.com").await.unwrap();
        assert!(conn1.is_active);

        // 归还连接
        pool.return_connection("example.com", conn1).await.unwrap();

        // 再次获取连接，应该复用之前的连接
        let conn2 = pool.get_connection("example.com").await.unwrap();
        assert!(conn2.is_active);
    }

    #[tokio::test]
    async fn test_adaptive_cache_manager() {
        let manager = AdaptiveCacheManager::new();

        // 测试HTTP缓存
        let test_data = b"test response data".to_vec();
        manager
            .cache_http_response("http://example.com", test_data.clone())
            .await
            .unwrap();

        let cached_data = manager.get_cached_http_response("http://example.com").await;
        assert_eq!(cached_data, Some(test_data));

        // 测试元数据缓存
        let metadata = serde_json::json!({"title": "test", "duration": 120});
        manager
            .cache_metadata("video_123", metadata.clone())
            .await
            .unwrap();

        let cached_metadata = manager.get_cached_metadata("video_123").await;
        assert_eq!(cached_metadata, Some(metadata));
    }
}
