//! Tauri command 层：全部 thin wrapper，业务逻辑一律经 temporule-core。
//!
//! 错误一律字符串化中文提示上抛前端；[`StoreError::SaveConflict`] 固定映射为
//! [`SAVE_CONFLICT_MSG`]（前端按文案识别保存冲突并强制重载）。

use chrono::{DateTime, Local, NaiveTime};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, WebviewWindow};
use temporule_core::engine;
use temporule_core::model::{
    Delivery, HistoryEntry, HistoryStatus, PopupAlert, Reminder, Repeat, Schedule, SnoozeRecord,
    TimeWindow,
};
use temporule_core::settings::AppSettings;
use temporule_core::store::StoreError;

use crate::store_state;

/// SaveConflict 的前端提示文案（前端按此识别保存冲突）。
const SAVE_CONFLICT_MSG: &str = "数据已被外部修改，请重试";

/// `history` 命令默认返回条数。
const DEFAULT_HISTORY_LIMIT: usize = 200;

/// 提醒列表行（仪表盘卡片与主窗口表格共用）。
#[derive(Debug, Clone, Serialize)]
pub struct ReminderView {
    /// 提醒标识。
    pub id: String,
    /// 标题。
    pub title: String,
    /// 备注。
    pub note: Option<String>,
    /// 中文排程摘要（[`Reminder::describe_schedule`]）。
    pub schedule_desc: String,
    /// 是否启用。
    pub enabled: bool,
    /// 下次触发时刻（RFC3339；禁用或 8 天内无触发为 `null`，前端显示「—」）。
    pub next_fire: Option<DateTime<Local>>,
    /// 错过是否补发。
    pub catch_up: bool,
    /// 触达方式：`"notification"` | `"popup"`。
    pub delivery_type: String,
}

/// 新增/编辑表单载荷（与前端表单字段一一对应，时间文本为 `HH:MM`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderPayload {
    /// 标题（非空白）。
    pub title: String,
    /// 备注（可空）。
    pub note: Option<String>,
    /// 重复规则：`"daily"` | `"weekly"`。
    pub repeat: String,
    /// 每周重复的星期（`"mon"`…`"sun"`，weekly 时非空）。
    pub weekdays: Vec<String>,
    /// 排程模式：`"every"`（间隔 + 时间窗）| `"at"`（固定时刻）。
    pub schedule_mode: String,
    /// 间隔分钟数（every 模式，≥ 1）。
    pub interval_minutes: u32,
    /// 时间窗起点（every 模式）。
    pub window_start: String,
    /// 时间窗终点（every 模式，须晚于起点）。
    pub window_end: String,
    /// 固定时刻列表（at 模式非空）。
    pub times: Vec<String>,
    /// 触达方式：`"notification"` | `"popup"`。
    pub delivery: String,
    /// 系统通知是否带提示音。
    pub sound: bool,
    /// 弹窗行为：`"auto"`（自动消失）| `"force"`（强制处理）。
    pub popup_mode: String,
    /// 弹窗自动消失秒数（popup/auto 模式）。
    pub auto_dismiss_seconds: u32,
    /// 「稍后」默认分钟数。
    pub snooze_minutes: u32,
    /// 错过是否补发。
    pub catch_up: bool,
}

/// 提醒完整字段（编辑表单预填，与 [`ReminderPayload`] 同构 + 身份字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderDetail {
    /// 提醒标识。
    pub id: String,
    /// 是否启用。
    pub enabled: bool,
    /// 表单打开时的 store rev 快照（保存时作为 rev 守卫基线，防过期快照覆盖）。
    pub rev: u64,
    /// 表单字段。
    #[serde(flatten)]
    pub payload: ReminderPayload,
}

impl ReminderPayload {
    /// 组装 core 模型并强制模型不变量；编辑时沿用原 id / 创建时刻 / 启用状态。
    fn into_reminder(self, existing: Option<&Reminder>) -> Result<Reminder, String> {
        let repeat = match self.repeat.as_str() {
            "daily" => Repeat::Daily,
            "weekly" => Repeat::Weekly(
                self.weekdays
                    .iter()
                    .map(|day| parse_weekday(day))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            other => return Err(format!("重复规则无效：{other}")),
        };
        let schedule = match self.schedule_mode.as_str() {
            "every" => Schedule::Every {
                interval_minutes: self.interval_minutes,
                window: TimeWindow {
                    start: parse_time(&self.window_start, "时间窗起点")?,
                    end: parse_time(&self.window_end, "时间窗终点")?,
                },
            },
            "at" => Schedule::At {
                times: self
                    .times
                    .iter()
                    .map(|time| parse_time(time, "固定时刻"))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            other => return Err(format!("排程模式无效：{other}")),
        };
        let delivery = match self.delivery.as_str() {
            "notification" => Delivery::Notification { sound: self.sound },
            "popup" => Delivery::Popup {
                alert: match self.popup_mode.as_str() {
                    "auto" => PopupAlert::AutoDismiss {
                        seconds: self.auto_dismiss_seconds,
                    },
                    "force" => PopupAlert::RequireAction,
                    other => return Err(format!("弹窗模式无效：{other}")),
                },
            },
            other => return Err(format!("触达方式无效：{other}")),
        };
        let now = Local::now();
        let reminder = Reminder {
            id: existing.map_or_else(uuid::Uuid::new_v4, |r| r.id),
            title: self.title,
            note: self.note.filter(|note| !note.trim().is_empty()),
            repeat,
            schedule,
            delivery,
            snooze_minutes: self.snooze_minutes,
            catch_up: self.catch_up,
            enabled: existing.is_none_or(|r| r.enabled),
            created_at: existing.map_or(now, |r| r.created_at),
            updated_at: now,
        };
        reminder.validate().map_err(|err| err.to_string())?;
        Ok(reminder)
    }
}

/// 提醒全部（含列表行）。
#[tauri::command(rename_all = "snake_case")]
pub fn list() -> Result<Vec<ReminderView>, String> {
    let store = store_state::store()?;
    let data = store.load().map_err(err_string)?;
    let now = Local::now();
    Ok(data
        .reminders
        .iter()
        .map(|reminder| ReminderView {
            id: reminder.id.to_string(),
            title: reminder.title.clone(),
            note: reminder.note.clone(),
            schedule_desc: reminder.describe_schedule(),
            enabled: reminder.enabled,
            next_fire: if reminder.enabled {
                engine::next_fire(reminder, now)
            } else {
                None
            },
            catch_up: reminder.catch_up,
            delivery_type: match reminder.delivery {
                Delivery::Notification { .. } => "notification",
                Delivery::Popup { .. } => "popup",
            }
            .to_string(),
        })
        .collect())
}

/// 提醒完整字段（编辑表单预填，含表单会话的 rev 快照）。
#[tauri::command(rename_all = "snake_case")]
pub fn get(id: String) -> Result<ReminderDetail, String> {
    let id = parse_uuid(&id)?;
    let store = store_state::store()?;
    let data = store.load().map_err(err_string)?;
    let reminder = data
        .reminders
        .iter()
        .find(|reminder| reminder.id == id)
        .ok_or_else(|| format!("提醒不存在：{id}"))?;
    Ok(detail_of(reminder, data.rev))
}

/// 新增提醒（校验失败/存储冲突返回中文错误）。
///
/// `base_rev` 为表单打开时的 rev 快照：给出时用作保存基线（会话期间外部已写入 →
/// [`StoreError::SaveConflict`]），缺省按当前 rev 保存（无快照保护）。
#[tauri::command(rename_all = "snake_case")]
pub fn add(
    window: WebviewWindow,
    payload: ReminderPayload,
    base_rev: Option<u64>,
) -> Result<(), String> {
    let reminder = payload.into_reminder(None)?;
    let store = store_state::store()?;
    let mut data = store.load().map_err(err_string)?;
    data.reminders.push(reminder);
    store
        .save(base_rev.unwrap_or(data.rev), &data.reminders)
        .map_err(err_string)?;
    store_state::reminders_written(&window);
    Ok(())
}

/// 编辑提醒（表单全量替换，rev 守卫冲突映射为固定文案）。
///
/// `base_rev` 必须携带表单打开（`get`）时的 rev 快照：外部写入后保存即冲突，
/// 防止过期快照覆盖（规格 Major#5），前端按 [`SAVE_CONFLICT_MSG`] 提示并强制重载。
#[tauri::command(rename_all = "snake_case")]
pub fn update(
    window: WebviewWindow,
    id: String,
    payload: ReminderPayload,
    base_rev: u64,
) -> Result<(), String> {
    let id = parse_uuid(&id)?;
    let store = store_state::store()?;
    let mut data = store.load().map_err(err_string)?;
    let pos = data
        .reminders
        .iter()
        .position(|reminder| reminder.id == id)
        .ok_or_else(|| format!("提醒不存在：{id}"))?;
    let reminder = payload.into_reminder(Some(&data.reminders[pos]))?;
    data.reminders[pos] = reminder;
    store.save(base_rev, &data.reminders).map_err(err_string)?;
    store_state::reminders_written(&window);
    Ok(())
}

/// 删除提醒（不级联清理 snooze，由调度器清扫兜底）。
#[tauri::command(rename_all = "snake_case")]
pub fn remove(window: WebviewWindow, id: String) -> Result<(), String> {
    let id = parse_uuid(&id)?;
    let store = store_state::store()?;
    let mut data = store.load().map_err(err_string)?;
    let before = data.reminders.len();
    data.reminders.retain(|reminder| reminder.id != id);
    if data.reminders.len() == before {
        return Err(format!("提醒不存在：{id}"));
    }
    store.save(data.rev, &data.reminders).map_err(err_string)?;
    store_state::reminders_written(&window);
    Ok(())
}

/// 启用/禁用提醒（bump `updated_at`，重启用后错过扫描不回溯禁用期）。
#[tauri::command(rename_all = "snake_case")]
pub fn toggle(window: WebviewWindow, id: String, enabled: bool) -> Result<(), String> {
    let id = parse_uuid(&id)?;
    let store = store_state::store()?;
    let mut data = store.load().map_err(err_string)?;
    let reminder = data
        .reminders
        .iter_mut()
        .find(|reminder| reminder.id == id)
        .ok_or_else(|| format!("提醒不存在：{id}"))?;
    reminder.enabled = enabled;
    reminder.updated_at = Local::now();
    store.save(data.rev, &data.reminders).map_err(err_string)?;
    store_state::reminders_written(&window);
    Ok(())
}

/// 历史记录（时间序，最近 `limit` 条；缺省 200）。
#[tauri::command(rename_all = "snake_case")]
pub fn history(limit: Option<usize>) -> Result<Vec<HistoryEntry>, String> {
    store_state::store()?
        .read_history(limit.unwrap_or(DEFAULT_HISTORY_LIMIT))
        .map_err(err_string)
}

/// 读取应用设置。
#[tauri::command(rename_all = "snake_case")]
pub fn settings() -> Result<AppSettings, String> {
    store_state::store()?.load_settings().map_err(err_string)
}

/// 保存应用设置，并同步生效「开机自启」与「失焦隐藏」运行期行为。
#[tauri::command(rename_all = "snake_case")]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    // 先落登录项，失败不动 settings.json（UI 状态与磁盘保持一致）
    apply_autostart(&app, settings.launch_at_login)?;
    store_state::store()?
        .save_settings(&settings)
        .map_err(err_string)?;
    store_state::set_dashboard_hide_on_blur(settings.dashboard_hide_on_blur);
    Ok(())
}

/// 弹窗按钮动作：`done` 记已完成并关窗；`snooze` 按提醒的「稍后」分钟数挂 SnoozeRecord
/// 记已稍后并关窗。两者都先经弹窗注册表标记消费（关窗不再记 dismissed）。
///
/// `popup_id` 形如 `{reminder_id}:{seq}`，`seq` 为该次计划触发时刻的 Unix 毫秒
/// （用于还原 `scheduled_at`，解析失败回退当前时刻）。
/// `popup_id` 指向的提醒不存在（含格式非法/已删除）→ 静默成功（仍消费并关窗）。
#[tauri::command(rename_all = "snake_case")]
pub fn popup_action(app: AppHandle, popup_id: String, action: String) -> Result<(), String> {
    if action != "done" && action != "snooze" {
        return Err(format!("未知弹窗操作：{action}"));
    }
    let (reminder_id_raw, seq_raw) = popup_id.split_once(':').unwrap_or((popup_id.as_str(), ""));
    let Ok(reminder_id) = reminder_id_raw.parse::<uuid::Uuid>() else {
        return Ok(());
    };
    let scheduled_at = seq_raw
        .parse::<i64>()
        .ok()
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
        .map(|at| at.with_timezone(&Local))
        .unwrap_or_else(Local::now);

    {
        // Store 锁限定在本块内：注册表消费/关窗绝不持 Store 锁（与 Destroyed 回调保持锁序）
        let store = store_state::store()?;
        let data = store.load().map_err(err_string)?;
        let now = Local::now();
        // 契约：已删静默（下面统一消费并关窗）
        if let Some(reminder) = data.reminders.iter().find(|r| r.id == reminder_id) {
            if action == "done" {
                store
                    .append_history(&HistoryEntry {
                        id: uuid::Uuid::new_v4(),
                        reminder_id,
                        title: reminder.title.clone(),
                        scheduled_at,
                        status: HistoryStatus::Done,
                        at: now,
                    })
                    .map_err(err_string)?;
            } else {
                store
                    .add_snooze(&SnoozeRecord {
                        id: uuid::Uuid::new_v4(),
                        reminder_id,
                        due_at: now + chrono::Duration::minutes(i64::from(reminder.snooze_minutes)),
                        created_at: now,
                    })
                    .map_err(err_string)?;
                store
                    .append_history(&HistoryEntry {
                        id: uuid::Uuid::new_v4(),
                        reminder_id,
                        title: reminder.title.clone(),
                        scheduled_at,
                        status: HistoryStatus::Snoozed,
                        at: now,
                    })
                    .map_err(err_string)?;
            }
        }
    }
    crate::popup::consume(&app, reminder_id);
    Ok(())
}

/// 弹窗行为（弹窗视图用）：`{kind:"auto",seconds}` 或 `{kind:"force"}`。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PopupAlertView {
    /// 显示若干秒后自动消失。
    Auto {
        /// 自动消失秒数。
        seconds: u32,
    },
    /// 必须手动处理。
    Force,
}

/// 弹窗内容（`popup_detail` 返回）。
#[derive(Debug, Clone, Serialize)]
pub struct PopupDetailView {
    /// 标题。
    pub title: String,
    /// 备注（可空）。
    pub note: Option<String>,
    /// 弹窗行为。
    pub alert: PopupAlertView,
}

/// 弹窗内容（thin wrapper）：提醒不存在 / ID 非法返回空，由弹窗视图自行关窗。
#[tauri::command(rename_all = "snake_case")]
pub fn popup_detail(popup_id: String) -> Result<Option<PopupDetailView>, String> {
    let (reminder_id_raw, _) = popup_id.split_once(':').unwrap_or((popup_id.as_str(), ""));
    let Ok(reminder_id) = reminder_id_raw.parse::<uuid::Uuid>() else {
        return Ok(None);
    };
    let store = store_state::store()?;
    let data = store.load().map_err(err_string)?;
    let Some(reminder) = data.reminders.iter().find(|r| r.id == reminder_id) else {
        return Ok(None);
    };
    let alert = match &reminder.delivery {
        Delivery::Popup { alert } => match alert {
            PopupAlert::AutoDismiss { seconds } => PopupAlertView::Auto { seconds: *seconds },
            PopupAlert::RequireAction => PopupAlertView::Force,
        },
        // 弹窗在途时触达方式被改为通知：退化为强制处理，不自动关窗
        Delivery::Notification { .. } => PopupAlertView::Force,
    };
    Ok(Some(PopupDetailView {
        title: reminder.title.clone(),
        note: reminder.note.clone(),
        alert,
    }))
}

/// 创建/聚焦主窗口（仪表盘「打开主窗口」按钮）。
#[tauri::command(rename_all = "snake_case")]
pub fn show_main_window(app: AppHandle) -> Result<(), String> {
    crate::show_main_window(&app).map_err(|err| format!("无法打开主窗口：{err}"))?;
    Ok(())
}

/// 数据目录路径（设置页只读展示）。
#[tauri::command(rename_all = "snake_case")]
pub fn data_dir() -> Result<String, String> {
    Ok(store_state::data_dir()?.to_string_lossy().into_owned())
}

/// 当前 store rev（新增表单打开时取一次快照，作保存的 rev 守卫基线）。
#[tauri::command(rename_all = "snake_case")]
pub fn store_rev() -> Result<u64, String> {
    Ok(store_state::store()?.load().map_err(err_string)?.rev)
}

/// 请求（或在设置页重试请求）系统通知权限，返回 `"granted"` / `"denied"`。
///
/// 请求失败上抛错误；被拒不报错，由调度器继续只记 history（不发系统通知）。
#[tauri::command(rename_all = "snake_case")]
pub fn request_notification_permission(app: AppHandle) -> Result<String, String> {
    crate::scheduler::request_notification_permission(&app)
}

/// 在访达中打开数据目录（与 CLI 的 `temporule open` 同为 `open` 命令调用）。
#[tauri::command(rename_all = "snake_case")]
pub fn open_data_dir() -> Result<(), String> {
    let dir = store_state::data_dir()?;
    std::process::Command::new("open")
        .arg(&dir)
        .status()
        .map_err(|err| format!("无法打开数据目录：{err}"))
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err("无法打开数据目录".to_string())
            }
        })
}

/// `StoreError` → 前端可读中文提示；SaveConflict 映射为固定冲突文案。
fn err_string(err: StoreError) -> String {
    match err {
        StoreError::SaveConflict { .. } => SAVE_CONFLICT_MSG.to_string(),
        other => other.to_string(),
    }
}

/// 解析提醒标识（无效标识返回中文错误）。
fn parse_uuid(raw: &str) -> Result<uuid::Uuid, String> {
    raw.parse().map_err(|_| format!("无效的提醒标识：{raw}"))
}

/// 解析 `HH:MM` / `HH:MM:SS` 时刻文本。
fn parse_time(raw: &str, field: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(raw, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(raw, "%H:%M:%S"))
        .map_err(|_| format!("{field}格式应为 HH:MM，实际为：{raw}"))
}

/// 解析星期短名（`"mon"`…`"sun"`，兼容英文全名，大小写不敏感）。
fn parse_weekday(raw: &str) -> Result<chrono::Weekday, String> {
    use chrono::Weekday;
    match raw.to_ascii_lowercase().as_str() {
        "mon" | "monday" => Ok(Weekday::Mon),
        "tue" | "tuesday" => Ok(Weekday::Tue),
        "wed" | "wednesday" => Ok(Weekday::Wed),
        "thu" | "thursday" => Ok(Weekday::Thu),
        "fri" | "friday" => Ok(Weekday::Fri),
        "sat" | "saturday" => Ok(Weekday::Sat),
        "sun" | "sunday" => Ok(Weekday::Sun),
        _ => Err(format!("无法识别的星期：{raw}")),
    }
}

/// 星期序列化短名（与 core 的 `mon`…`"sun"` 对齐）。
fn weekday_short(day: chrono::Weekday) -> &'static str {
    use chrono::Weekday;
    match day {
        Weekday::Mon => "mon",
        Weekday::Tue => "tue",
        Weekday::Wed => "wed",
        Weekday::Thu => "thu",
        Weekday::Fri => "fri",
        Weekday::Sat => "sat",
        Weekday::Sun => "sun",
    }
}

/// core 模型 → 表单预填视图（`rev` = 表单打开时的 store rev 快照）。
fn detail_of(reminder: &Reminder, rev: u64) -> ReminderDetail {
    let (repeat, weekdays) = match &reminder.repeat {
        Repeat::Daily => ("daily".to_string(), Vec::new()),
        Repeat::Weekly(days) => (
            "weekly".to_string(),
            days.iter()
                .map(|day| weekday_short(*day).to_string())
                .collect(),
        ),
    };
    let (schedule_mode, interval_minutes, window_start, window_end, times) =
        match &reminder.schedule {
            Schedule::Every {
                interval_minutes,
                window,
            } => (
                "every".to_string(),
                *interval_minutes,
                window.start.format("%H:%M").to_string(),
                window.end.format("%H:%M").to_string(),
                Vec::new(),
            ),
            Schedule::At { times } => (
                "at".to_string(),
                0,
                String::new(),
                String::new(),
                times
                    .iter()
                    .map(|time| time.format("%H:%M").to_string())
                    .collect(),
            ),
        };
    let (delivery, sound, popup_mode, auto_dismiss_seconds) = match &reminder.delivery {
        Delivery::Notification { sound } => {
            ("notification".to_string(), *sound, "auto".to_string(), 30)
        }
        Delivery::Popup { alert } => match alert {
            PopupAlert::AutoDismiss { seconds } => {
                ("popup".to_string(), false, "auto".to_string(), *seconds)
            }
            PopupAlert::RequireAction => ("popup".to_string(), false, "force".to_string(), 0),
        },
    };
    ReminderDetail {
        id: reminder.id.to_string(),
        enabled: reminder.enabled,
        rev,
        payload: ReminderPayload {
            title: reminder.title.clone(),
            note: reminder.note.clone(),
            repeat,
            weekdays,
            schedule_mode,
            interval_minutes,
            window_start,
            window_end,
            times,
            delivery,
            sound,
            popup_mode,
            auto_dismiss_seconds,
            snooze_minutes: reminder.snooze_minutes,
            catch_up: reminder.catch_up,
        },
    }
}

/// 开机自启应用/取消（失败提示用户把 App 放到 /Applications）。
fn apply_autostart(app: &AppHandle, enable: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt as _;
    let launcher = app.autolaunch();
    if enable {
        launcher.enable()
    } else {
        launcher.disable()
    }
    .map_err(|err| format!("更新开机自启失败（请确保 App 位于 /Applications）：{err}"))
}
