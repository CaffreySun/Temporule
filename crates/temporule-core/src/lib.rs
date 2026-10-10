//! Temporule（拾律）调度引擎与存储核心。
//!
//! 依赖方向约束：本 crate 不得依赖任何 GUI / 系统 UI / 通知 crate。
//! 接缝（测试边界）：`model` 公开类型与校验、`engine` 公开纯函数、`store::Store` 公开方法。

pub mod engine;
pub mod model;
pub mod settings;
pub mod store;
