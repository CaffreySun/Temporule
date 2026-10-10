//! 应用设置（`settings.json`）。
//!
//! 设置是 GUI 偏好开关，与提醒数据分离存储；读写经 [`crate::store::Store`] 的
//! `load_settings` / `save_settings`（原子写，见存储层文档）。

use serde::{Deserialize, Serialize};

/// 应用设置：GUI 偏好开关。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSettings {
    /// 开机自启。
    pub launch_at_login: bool,
    /// 新提醒的默认提示音（系统通知触达）。
    pub default_sound: bool,
    /// 仪表盘失焦自动隐藏（默认开启）。
    pub dashboard_hide_on_blur: bool,
}

impl Default for AppSettings {
    /// 默认设置：不开机自启、通知带声音、仪表盘失焦隐藏。
    fn default() -> Self {
        Self {
            launch_at_login: false,
            default_sound: true,
            dashboard_hide_on_blur: true,
        }
    }
}
