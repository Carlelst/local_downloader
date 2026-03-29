/// 领域层 - 核心业务逻辑和实体
///
/// 该层负责：
/// - 定义核心业务实体和值对象
/// - 实现业务规则和逻辑
/// - 定义领域服务接口
/// - 管理业务不变量
pub mod entities;
pub mod errors;
pub mod services;
pub mod value_objects;

pub use entities::*;
pub use errors::*;
pub use services::*;
pub use value_objects::*;
