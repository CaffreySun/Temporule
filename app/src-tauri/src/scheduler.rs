//! 调度循环（Rust 侧 async）：每分钟对齐边界的 tick 四步循环 + 启动错过扫描 + 通知分发。
//!
//! 契约（`.task_spec/temporule-v1.md` GUI 节 + 修订记录轮 3 的时延二分）：
//! 1. 间隙处理：enabled 提醒取 `occurrences(rem, last_tick, now)`——`t ≥ now − P`（P=60s）准点
//!    无条件 fired；`t < now − P` 为间隙，按 catch_up 补发或记 missed（睡眠间隙天然覆盖）；
//! 2. snooze 消费：`snoozes_due(last_tick, now)` 同时延二分；两种结局都 remove_snooze
//!    （consume-once）；指向已删提醒的记录静默移除并记 log；
//! 3. 过期 snooze 清扫：`due ≤ last_tick` 原子移除后按 catch_up 补发或记 missed；
//! 4. 触发分发：history(fired) + 按 delivery 发通知 / 经弹窗注册表建弹窗。
//!
//! tick 末 history 截断检查（唯一截断写者 = 调度器）+ 心跳 `state.json.last_tick = now`。
//! 启动时按 `missed_scan_from` 做错过扫描（同一时延二分）+ 一次过期 snooze 清扫。
//!
//! 锁序纪律：Store 锁绝不跨分发（窗口创建会同步等主线程，跨锁等待会与 Destroyed 回调死锁）；
//! history 写入逐条短持锁。

use std::sync::atomic::{AtomicU8, Ordering};

use chrono::{DateTime, Duration, Local, TimeZone, Timelike};
use tauri::AppHandle;
use temporule_core::engine;
use temporule_core::model::{Delivery, HistoryEntry, HistoryStatus, Reminder};
use temporule_core::store::Store;
use uuid::Uuid;

use crate::popup::{self, PopupSpec};
use crate::store_state;

/// tick 周期 P（秒）：时延二分阈值——`t ≥ now − P` 视为准点。
const TICK_PERIOD_SECS: i64 = 60;

/// 系统通知默认音效（macOS 走 NSUserNotification 默认音名，其余平台 `default`）。
#[cfg(target_os = "macos")]
const DEFAULT_SOUND: &str = "NSUserNotificationDefaultSoundName";
#[cfg(not(target_os = "macos"))]
const DEFAULT_SOUND: &str = "default";

/// 边界对齐余量：tick 目标点 = 整分边界 + 该余量。
///
/// 唤醒相对墙钟存在毫秒级偏差（时钟微调），若恰好瞄准边界，边界时刻的 occurrence 会差
/// 零点几毫秒落不进 `(last_tick, now]`，被推迟到下一分钟才处理。余量确保边界时刻准时入窗。
const BOUNDARY_EPSILON: Duration = Duration::milliseconds(25);

/// 一次待分发触发（提醒索引指向当次加载快照 + 计划触发时刻）。
struct Fire {
    /// `data.reminders` 中的索引。
    reminder_idx: usize,
    /// 计划触发时刻（occurrence 或 snooze due_at）。
    scheduled_at: DateTime<Local>,
}

/// 一趟扫描的上下文：Store 访问 + 时延二分阈值 + 待分发集合。
struct PassCtx<'a> {
    /// 存储句柄（调用方持锁，本上下文只借读写）。
    store: &'a Store,
    /// 时延二分阈值 `now − P`。
    gap: DateTime<Local>,
    /// 本趟扫描上界（history 的 `at`）。
    now: DateTime<Local>,
    /// 待分发触发（步骤 4 统一处理）。
    fires: Vec<Fire>,
}

impl PassCtx<'_> {
    /// 时延二分：`t ≥ gap` 准点无条件触发；间隙按 catch_up 补发或记 missed。
    fn classify(
        &mut self,
        reminder_idx: usize,
        reminder: &Reminder,
        scheduled_at: DateTime<Local>,
        context: &str,
    ) -> Result<(), String> {
        let title = &reminder.title;
        if scheduled_at >= self.gap {
            eprintln!("[temporule-sched] {context}准点触发：{title} @ {scheduled_at}");
            self.fires.push(Fire {
                reminder_idx,
                scheduled_at,
            });
        } else if reminder.catch_up {
            eprintln!("[temporule-sched] {context}错过补发：{title} @ {scheduled_at}");
            self.fires.push(Fire {
                reminder_idx,
                scheduled_at,
            });
        } else {
            eprintln!("[temporule-sched] {context}错过未补发，记 missed：{title} @ {scheduled_at}");
            self.append_missed(reminder, scheduled_at)?;
        }
        Ok(())
    }

    /// 过期 snooze 清扫：`due ≤ bound` 的记录原子移除后按 catch_up 补发或记 missed；
    /// 指向已删提醒的记录静默移除并记 log。
    fn sweep_expired(
        &mut self,
        reminders: &[Reminder],
        bound: DateTime<Local>,
    ) -> Result<(), String> {
        // core 公开 API 只有半开区间查询，下界取 Unix 纪元（snooze 不可能早于它）
        let floor = Local
            .timestamp_opt(0, 0)
            .single()
            .ok_or_else(|| "无法计算 snooze 查询下界".to_string())?;
        let due = self
            .store
            .snoozes_due(floor, bound)
            .map_err(|err| err.to_string())?;
        for record in due {
            match reminders
                .iter()
                .enumerate()
                .find(|(_, reminder)| reminder.id == record.reminder_id)
            {
                None => {
                    eprintln!(
                        "[temporule-sched] 过期 snooze 指向已删提醒，静默移除：{}",
                        record.reminder_id
                    );
                }
                Some((reminder_idx, reminder)) => {
                    if reminder.catch_up {
                        eprintln!(
                            "[temporule-sched] 过期 snooze 补发：{} @ {}",
                            reminder.title, record.due_at
                        );
                        self.fires.push(Fire {
                            reminder_idx,
                            scheduled_at: record.due_at,
                        });
                    } else {
                        eprintln!(
                            "[temporule-sched] 过期 snooze 未补发，记 missed：{} @ {}",
                            reminder.title, record.due_at
                        );
                        self.append_missed(reminder, record.due_at)?;
                    }
                }
            }
            // consume-once：两种结局都原子移除
            self.store
                .remove_snooze(record.id)
                .map_err(|err| err.to_string())?;
        }
        Ok(())
    }

    /// 记 history(missed)。
    fn append_missed(
        &self,
        reminder: &Reminder,
        scheduled_at: DateTime<Local>,
    ) -> Result<(), String> {
        self.store
            .append_history(&HistoryEntry {
                id: Uuid::new_v4(),
                reminder_id: reminder.id,
                title: reminder.title.clone(),
                scheduled_at,
                status: HistoryStatus::Missed,
                at: self.now,
            })
            .map_err(|err| err.to_string())
    }
}

/// 启动调度：先做启动错过扫描 + 过期 snooze 清扫并落心跳，再进入每分钟对齐边界的 tick 循环。
pub fn start(app: AppHandle) {
    let now = Local::now();
    if let Err(err) = startup_scan(&app, now) {
        eprintln!("[temporule-sched] 启动扫描失败：{err}");
    }
    let mut last_tick = now;
    tauri::async_runtime::spawn(async move {
        loop {
            let wait = until_next_minute(Local::now());
            tokio::time::sleep(wait.to_std().unwrap_or(std::time::Duration::from_secs(60))).await;
            let now = Local::now();
            if let Err(err) = tick_pass(&app, last_tick, now) {
                eprintln!("[temporule-sched] tick 处理失败：{err}");
            }
            // 无论成败都推进心跳，避免同一区间在运行内被重复分发
            last_tick = now;
        }
    });
}

/// 距下一个整分边界（+ [`BOUNDARY_EPSILON`]）的时长（正数；恰在边界时为一整周期）。
fn until_next_minute(now: DateTime<Local>) -> Duration {
    let sub_nanos = i64::from(now.timestamp_subsec_nanos());
    Duration::seconds(TICK_PERIOD_SECS - i64::from(now.second())) - Duration::nanoseconds(sub_nanos)
        + BOUNDARY_EPSILON
}

/// 启动扫描：per-reminder `missed_scan_from` 下界的错过处理（同一时延二分）
/// + 一次过期 snooze 清扫（`due ≤ now`，即 App 未运行期间到点的全部记录）+ 落心跳。
///
/// 心跳在本趟末写入（`state.json` 缺失时 last_tick 取启动时刻 → 无错过扫描），
/// 使随后 tick 从扫描上界继续，不重复分发同一区间。
fn startup_scan(app: &AppHandle, now: DateTime<Local>) -> Result<(), String> {
    let (reminders, fires) = {
        let store = store_state::store()?;
        let last_tick = store.load_state().unwrap_or(now);
        let data = store.load().map_err(|err| err.to_string())?;
        let mut ctx = PassCtx {
            store: &store,
            gap: now - Duration::seconds(TICK_PERIOD_SECS),
            now,
            fires: Vec::new(),
        };
        for (idx, reminder) in data
            .reminders
            .iter()
            .enumerate()
            .filter(|(_, reminder)| reminder.enabled)
        {
            let from = engine::missed_scan_from(reminder, last_tick, now);
            for scheduled_at in engine::occurrences(reminder, from, now) {
                ctx.classify(idx, reminder, scheduled_at, "启动扫描，")?;
            }
        }
        ctx.sweep_expired(&data.reminders, now)?;
        (data.reminders, ctx.fires)
    };
    dispatch_fires(app, &reminders, &fires, now)?;
    finish_pass(now)
}

/// tick 四步（顺序固定）：间隙处理 → snooze 消费 → 过期 snooze 清扫 → 触发分发；
/// 末尾 history 截断检查 + 心跳。
fn tick_pass(
    app: &AppHandle,
    last_tick: DateTime<Local>,
    now: DateTime<Local>,
) -> Result<(), String> {
    let (reminders, fires) = {
        let store = store_state::store()?;
        let data = store.load().map_err(|err| err.to_string())?;
        let mut ctx = PassCtx {
            store: &store,
            gap: now - Duration::seconds(TICK_PERIOD_SECS),
            now,
            fires: Vec::new(),
        };
        // 步骤 1 间隙处理
        for (idx, reminder) in data
            .reminders
            .iter()
            .enumerate()
            .filter(|(_, reminder)| reminder.enabled)
        {
            for scheduled_at in engine::occurrences(reminder, last_tick, now) {
                ctx.classify(idx, reminder, scheduled_at, "")?;
            }
        }
        // 步骤 2 snooze 消费（时延二分，两种结局都 consume-once）
        let due = store
            .snoozes_due(last_tick, now)
            .map_err(|err| err.to_string())?;
        for record in due {
            match data
                .reminders
                .iter()
                .enumerate()
                .find(|(_, reminder)| reminder.id == record.reminder_id)
            {
                None => {
                    eprintln!(
                        "[temporule-sched] snooze 指向已删提醒，静默移除：{}",
                        record.reminder_id
                    );
                }
                Some((idx, reminder)) => {
                    ctx.classify(idx, reminder, record.due_at, "snooze 到点，")?;
                }
            }
            store
                .remove_snooze(record.id)
                .map_err(|err| err.to_string())?;
        }
        // 步骤 3 过期 snooze 清扫（due ≤ last_tick 的记录）
        ctx.sweep_expired(&data.reminders, last_tick)?;
        (data.reminders, ctx.fires)
    };
    // 步骤 4 触发分发
    dispatch_fires(app, &reminders, &fires, now)?;
    finish_pass(now)
}

/// 步骤 4 触发分发：逐条 history(fired)（Store 锁短持）+ 发通知 / 经弹窗注册表建弹窗（不持任何锁）。
fn dispatch_fires(
    app: &AppHandle,
    reminders: &[Reminder],
    fires: &[Fire],
    now: DateTime<Local>,
) -> Result<(), String> {
    for fire in fires {
        let reminder = &reminders[fire.reminder_idx];
        {
            let store = store_state::store()?;
            store
                .append_history(&HistoryEntry {
                    id: Uuid::new_v4(),
                    reminder_id: reminder.id,
                    title: reminder.title.clone(),
                    scheduled_at: fire.scheduled_at,
                    status: HistoryStatus::Fired,
                    at: now,
                })
                .map_err(|err| err.to_string())?;
        }
        match &reminder.delivery {
            Delivery::Notification { sound } => send_notification(app, reminder, *sound),
            Delivery::Popup { alert } => popup::show(
                app,
                PopupSpec {
                    reminder_id: reminder.id,
                    title: reminder.title.clone(),
                    scheduled_at: fire.scheduled_at,
                    alert: alert.clone(),
                },
            ),
        }
    }
    Ok(())
}

/// 趟末收尾：history 截断检查（唯一截断写者 = 调度器）+ 心跳 `state.json.last_tick = now`。
fn finish_pass(now: DateTime<Local>) -> Result<(), String> {
    let store = store_state::store()?;
    store.truncate_history().map_err(|err| err.to_string())?;
    store.save_state(now).map_err(|err| err.to_string())
}

/// 通知权限状态：尚未请求过。
const NOTIFY_UNREQUESTED: u8 = 0;
/// 通知权限状态：已授权，正常分发系统通知。
const NOTIFY_GRANTED: u8 = 1;
/// 通知权限状态：被拒或请求失败，跳过系统通知（设置页可重试请求）。
const NOTIFY_BLOCKED: u8 = 2;

/// 通知权限缓存（调度器分发与设置页重试共享）：[`NOTIFY_UNREQUESTED`] / [`NOTIFY_GRANTED`] / [`NOTIFY_BLOCKED`]。
static NOTIFY_PERM: AtomicU8 = AtomicU8::new(NOTIFY_UNREQUESTED);

/// 请求（或在设置页重试请求）系统通知权限，返回 `"granted"` / `"denied"`。
///
/// 授予即恢复调度器的通知分发（清零被拒状态）；被拒保持跳过通知、仅记 history；
/// 请求本身失败上抛错误（状态同样置为被拒，设置页可再重试）。
pub fn request_notification_permission(app: &AppHandle) -> Result<String, String> {
    use tauri_plugin_notification::{NotificationExt, PermissionState};
    match app.notification().request_permission() {
        Ok(PermissionState::Granted) => {
            NOTIFY_PERM.store(NOTIFY_GRANTED, Ordering::Relaxed);
            Ok("granted".to_string())
        }
        Ok(state) => {
            NOTIFY_PERM.store(NOTIFY_BLOCKED, Ordering::Relaxed);
            eprintln!("[temporule-sched] 通知权限被拒（{state:?}），跳过系统通知，仅记录 history");
            Ok("denied".to_string())
        }
        Err(err) => {
            NOTIFY_PERM.store(NOTIFY_BLOCKED, Ordering::Relaxed);
            Err(format!("通知权限请求失败：{err}"))
        }
    }
}

/// 首次触发前请求一次通知权限；被拒仅记 log 并跳过通知（history 已在分发前写入）。
fn notify_permitted(app: &AppHandle) -> bool {
    if NOTIFY_PERM.load(Ordering::Relaxed) == NOTIFY_UNREQUESTED {
        // 只在首次触发时请求一次；结果被拒不打断调度，恢复走设置页的重试入口
        if let Err(err) = request_notification_permission(app) {
            eprintln!("[temporule-sched] {err}，跳过系统通知");
        }
    }
    NOTIFY_PERM.load(Ordering::Relaxed) == NOTIFY_GRANTED
}

/// 系统通知分发（准点 / 补发同路径）；`sound = true` 时带提示音。
fn send_notification(app: &AppHandle, reminder: &Reminder, sound: bool) {
    use tauri_plugin_notification::NotificationExt;
    if !notify_permitted(app) {
        return;
    }
    let mut builder = app.notification().builder().title(reminder.title.clone());
    if let Some(note) = &reminder.note {
        builder = builder.body(note.clone());
    }
    if sound {
        builder = builder.sound(DEFAULT_SOUND);
    }
    if let Err(err) = builder.show() {
        eprintln!("[temporule-sched] 系统通知发送失败：{err}");
    }
}
