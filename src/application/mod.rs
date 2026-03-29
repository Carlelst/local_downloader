pub mod monitoring_service;
pub mod service_factory;
/// 应用层 - 业务逻辑协调和工作流管理
///
/// 该层负责：
/// - 协调领域层和基础设施层
/// - 实现业务用例和工作流
/// - 管理事务和状态
/// - 提供应用服务接口
pub mod services;
pub mod validation;
pub mod workflows;

pub use monitoring_service::{
    DefaultMonitoringService, MonitoringServiceConfig, MonitoringSummary, ServiceHealth,
    SystemOverview, TaskMonitoringConfig, TaskOverview,
};
pub use service_factory::{
    create_default_service_factory, create_legacy_adapter, LegacyServiceAdapter, ServiceFactory,
};
pub use services::*;
pub use validation::*;
pub use workflows::*;
