//! 弹窗注册表（防堆积）：同 reminder 单实例替换、全局存活上限、FIFO 排队递补。
//!
//! 契约要点（`.task_spec/temporule-v1.md` GUI 节）：
//! - 窗口 label `popup-{reminder_id}`，~360×140，右下纵向堆叠，alwaysOnTop / skipTaskbar /
//!   closable / 不可最小化 / 无系统标题栏（标题区前端自绘）/ 不抢焦点（focusable(false)）；
//!   同 reminder 二次触发复用窗口注入新内容并重置 AutoDismiss 计时；
//! - 全局同时存活上限 [`MAX_ALIVE`]，超出 FIFO 排队，窗口关闭时递补；
//! - AutoDismiss 到时由 Rust 关窗；窗口 Destroyed 时未被 done/snooze 消费过才记
//!   history(dismissed)（done/snooze 经 [`consume`] 先标记再关窗）。
//!
//! 锁序纪律：注册表锁只护内存状态；任何窗口操作（建窗会同步等主线程）与 Store 写入
//! 都在放锁后进行，避免与 Destroyed 回调跨锁互等。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Local};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use temporule_core::model::{HistoryEntry, HistoryStatus, PopupAlert};
use uuid::Uuid;

use crate::store_state;

/// 弹窗替换事件名：payload 携带新 popup_id，前端据此换内容并重置倒计时。
pub const POPUP_REPLACE_EVENT: &str = "popup-replace";

/// 全局同时存活弹窗上限，超出 FIFO 排队。
const MAX_ALIVE: usize = 5;

/// 弹窗窗口尺寸（逻辑像素）。
const POPUP_SIZE: (f64, f64) = (360.0, 140.0);

/// 弹窗纵向堆叠间距（逻辑像素）。
const POPUP_GAP: f64 = 8.0;

/// 应用退出中：退出清窗口不产生 dismissed / 递补。
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// `popup-replace` 事件载荷。
#[derive(Debug, Clone, Serialize)]
struct ReplacePayload {
    /// 当前 popup_id（前端据此拉取内容）。
    popup_id: String,
}

/// 一次弹窗触发的内容快照（替换 / 排队时整体顶替）。
#[derive(Debug, Clone)]
pub struct PopupSpec {
    /// 所属提醒。
    pub reminder_id: Uuid,
    /// 标题快照（history 的 title 也取此值）。
    pub title: String,
    /// 该次计划触发时刻（popup_id 的 seq 与 history 的 scheduled_at 由此而来）。
    pub scheduled_at: DateTime<Local>,
    /// 弹窗行为（触发时配置快照，AutoDismiss 计时依据）。
    pub alert: PopupAlert,
}

/// 存活弹窗条目。
struct AlivePopup {
    /// 窗口 label。
    label: String,
    /// 当前内容（同 reminder 二次触发时顶替）。
    spec: PopupSpec,
    /// 替换代数：每次替换 +1，旧代 AutoDismiss 计时据此作废（计时重置）。
    generation: u64,
    /// 是否已被 done/snooze 消费（消费后关窗不再记 dismissed）。
    consumed: bool,
}

/// 注册表：存活弹窗（FIFO）+ 等待递补队列（FIFO）。
struct Registry {
    alive: VecDeque<AlivePopup>,
    pending: VecDeque<PopupSpec>,
    next_generation: u64,
}

impl Registry {
    /// 取一个新替换代数。
    fn take_generation(&mut self) -> u64 {
        let generation = self.next_generation;
        self.next_generation += 1;
        generation
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    alive: VecDeque::new(),
    pending: VecDeque::new(),
    next_generation: 1,
});

/// 取注册表锁（中毒时恢复继续用，弹窗状态不因单次 panic 失效）。
fn lock_registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 弹窗窗口 label：`popup-{reminder_id}`。
fn label_of(reminder_id: Uuid) -> String {
    format!("popup-{reminder_id}")
}

/// 弹窗 ID：`{reminder_id}:{scheduled_at 的 Unix 毫秒}`（popup_action 据 seq 还原 scheduled_at）。
fn popup_id(spec: &PopupSpec) -> String {
    format!(
        "{}:{}",
        spec.reminder_id,
        spec.scheduled_at.timestamp_millis()
    )
}

/// show 的执行计划（注册表决策在锁内完成，窗口操作一律放锁后做）。
enum Plan {
    /// 新建窗口（条目已登记，需建窗）。
    Build { spec: PopupSpec, generation: u64 },
    /// 复用已有窗口注入新内容。
    Replace {
        label: String,
        spec: PopupSpec,
        generation: u64,
    },
    /// 已排队 / 排队中只换内容，无需窗口动作。
    Pending,
}

/// 触发一次弹窗：同 reminder 二次触发替换旧窗；达到存活上限 FIFO 排队；否则建窗。
pub fn show(app: &AppHandle, spec: PopupSpec) {
    let plan = {
        let mut reg = lock_registry();
        let label = label_of(spec.reminder_id);
        if let Some(pos) = reg.alive.iter().position(|entry| entry.label == label) {
            // 同 reminder 已有存活窗口 → 替换（复用窗口，AutoDismiss 计时随新代数重置）
            let generation = reg.take_generation();
            let entry = &mut reg.alive[pos];
            entry.spec = spec.clone();
            entry.consumed = false;
            entry.generation = generation;
            Plan::Replace {
                label,
                spec,
                generation,
            }
        } else if let Some(pending) = reg
            .pending
            .iter_mut()
            .find(|queued| queued.reminder_id == spec.reminder_id)
        {
            eprintln!(
                "[temporule-sched] 弹窗排队中，内容替换为最新触发：{}",
                spec.title
            );
            *pending = spec;
            Plan::Pending
        } else if reg.alive.len() >= MAX_ALIVE {
            eprintln!(
                "[temporule-sched] 弹窗存活已达上限 {MAX_ALIVE}，FIFO 排队：{}",
                spec.title
            );
            reg.pending.push_back(spec);
            Plan::Pending
        } else {
            let generation = reg.take_generation();
            reg.alive.push_back(AlivePopup {
                label,
                spec: spec.clone(),
                generation,
                consumed: false,
            });
            Plan::Build { spec, generation }
        }
    };
    match plan {
        Plan::Build { spec, generation } => build_window(app, spec, generation),
        Plan::Replace {
            label,
            spec,
            generation,
        } => replace_window(app, &label, &spec, generation),
        Plan::Pending => {}
    }
}

/// done/snooze 消费弹窗：标记后再关窗（Destroyed 里不再记 dismissed）。
pub fn consume(app: &AppHandle, reminder_id: Uuid) {
    let label = label_of(reminder_id);
    {
        let mut reg = lock_registry();
        if let Some(entry) = reg.alive.iter_mut().find(|entry| entry.label == label) {
            entry.consumed = true;
        }
    }
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.close();
    }
}

/// 应用退出中：退出清理的窗口不产生 dismissed / 递补。
pub fn mark_shutting_down() {
    SHUTTING_DOWN.store(true, Ordering::Relaxed);
}

/// 复用窗口注入新内容：发 [`POPUP_REPLACE_EVENT`]（换内容 + 重置倒计时）、换标题、重置计时（不抢焦点）。
fn replace_window(app: &AppHandle, label: &str, spec: &PopupSpec, generation: u64) {
    eprintln!(
        "[temporule-sched] 弹窗替换（同 reminder 不堆叠）：{}",
        spec.title
    );
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.set_title(&spec.title);
        let payload = ReplacePayload {
            popup_id: popup_id(spec),
        };
        let _ = window.emit_to(label, POPUP_REPLACE_EVENT, payload);
    } else {
        // 窗口尚在创建：建窗流程会在页面加载完成后补发当前内容
        eprintln!("[temporule-sched] 弹窗窗口未就绪，内容待页面加载后注入：{label}");
    }
    if let PopupAlert::AutoDismiss { seconds } = spec.alert {
        schedule_auto_dismiss(app, label, generation, seconds);
    }
}

/// 建弹窗窗口（条目已在注册表登记）。
fn build_window(app: &AppHandle, spec: PopupSpec, generation: u64) {
    let label = label_of(spec.reminder_id);
    let popup_id = popup_id(&spec);
    let mut builder = WebviewWindowBuilder::new(
        app,
        &label,
        // URL 必须每窗唯一（?view=popup 让前端按参数分发；pid + hash 的 popup_id 区分窗口），
        // 同 URL 双窗口会被 WKWebView 页面缓存串掉窗口元数据与页面内容
        WebviewUrl::App(
            format!("index.html?view=popup&pid={}#{popup_id}", spec.reminder_id).into(),
        ),
    )
    .title(&spec.title)
    .inner_size(POPUP_SIZE.0, POPUP_SIZE.1)
    .always_on_top(true)
    .skip_taskbar(true)
    .closable(true)
    .minimizable(false)
    // 无系统标题栏：标题区自绘（前端标题行可拖动 + 关闭按钮）
    // transparent：frameless 窗口 macOS 不自动圆角，靠透明窗口 + CSS 圆角实现
    .decorations(false)
    .transparent(true)
    // 不抢焦点：弹窗不成 key window（按钮仍可点击），用户正在输入的上下文不被打断
    .focusable(false)
    // 页面加载完成后补发当前 popup_id（同 tick 内二次触发可能早于前端监听注册）
    .on_page_load({
        let app = app.clone();
        let label = label.clone();
        move |_window, _payload| {
            if let Some(popup_id) = current_popup_id(&label) {
                let payload = ReplacePayload { popup_id };
                let _ = app.emit_to(&label, POPUP_REPLACE_EVENT, payload);
            }
        }
    });
    if let Some((x, y)) = popup_position(app, 0) {
        builder = builder.position(x, y);
    }
    let window = match builder.build() {
        Ok(window) => window,
        Err(err) => {
            eprintln!("[temporule-sched] 弹窗窗口创建失败：{err}");
            // 登记条目回滚，避免占住存活额度
            lock_registry().alive.retain(|entry| entry.label != label);
            relayout(app);
            return;
        }
    };
    // 关窗结局统一在 Destroyed 裁决（AutoDismiss 到时 / 手动关 / done-snooze 关窗都汇聚到这里）
    {
        let app = app.clone();
        let label = label.clone();
        window.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                on_window_destroyed(&app, &label);
            }
        });
    }
    relayout(app);
    if let PopupAlert::AutoDismiss { seconds } = spec.alert {
        schedule_auto_dismiss(app, &label, generation, seconds);
    }
    // 只显示不聚焦（focusable(false) 下 show 不会夺 key window）
    let _ = window.show();
}

/// 窗口销毁（任何关窗路径都汇聚到这里）：未被 done/snooze 消费过 → 记 dismissed；随后重排 + 递补。
fn on_window_destroyed(app: &AppHandle, label: &str) {
    if SHUTTING_DOWN.load(Ordering::Relaxed) {
        return;
    }
    let (record, queued) = {
        let mut reg = lock_registry();
        let Some(pos) = reg.alive.iter().position(|entry| entry.label == label) else {
            return;
        };
        let entry = reg.alive.remove(pos).expect("存活条目位置刚确认");
        let record = (!entry.consumed).then_some(entry.spec);
        (record, reg.pending.pop_front())
    };
    if let Some(spec) = record {
        eprintln!("[temporule-sched] 弹窗关闭记 dismissed：{}", spec.title);
        if let Err(err) = append_dismissed(&spec) {
            eprintln!("[temporule-sched] history 写入失败：{err}");
        }
    }
    relayout(app);
    if let Some(spec) = queued {
        eprintln!("[temporule-sched] 弹窗递补排队项：{}", spec.title);
        let generation = register_alive(&spec);
        build_window(app, spec, generation);
    }
}

/// 登记存活条目并取新代数（建窗前在锁内登记，防并发重复建同 label 窗口）。
fn register_alive(spec: &PopupSpec) -> u64 {
    let mut reg = lock_registry();
    let generation = reg.take_generation();
    reg.alive.push_back(AlivePopup {
        label: label_of(spec.reminder_id),
        spec: spec.clone(),
        generation,
        consumed: false,
    });
    generation
}

/// 注册表中该 label 当前的 popup_id（页面加载完成后补发用）。
fn current_popup_id(label: &str) -> Option<String> {
    let reg = lock_registry();
    reg.alive
        .iter()
        .find(|entry| entry.label == label)
        .map(|entry| popup_id(&entry.spec))
}

/// 关窗结局：记 history(dismissed)（标题 / 计划时刻取触发时快照）。
fn append_dismissed(spec: &PopupSpec) -> Result<(), String> {
    let store = store_state::store()?;
    store
        .append_history(&HistoryEntry {
            id: Uuid::new_v4(),
            reminder_id: spec.reminder_id,
            title: spec.title.clone(),
            scheduled_at: spec.scheduled_at,
            status: HistoryStatus::Dismissed,
            at: Local::now(),
        })
        .map_err(|err| err.to_string())
}

/// AutoDismiss 计时：到时关窗（dismissed 由 Destroyed 统一记）；替换后旧代数计时自动作废。
fn schedule_auto_dismiss(app: &AppHandle, label: &str, generation: u64, seconds: u32) {
    let app = app.clone();
    let label = label.to_string();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(u64::from(seconds))).await;
        let should_close = {
            let reg = lock_registry();
            reg.alive.iter().any(|entry| {
                entry.label == label && entry.generation == generation && !entry.consumed
            })
        };
        if should_close {
            if let Some(window) = app.get_webview_window(&label) {
                eprintln!("[temporule-sched] AutoDismiss 到时关窗：{label}");
                let _ = window.close();
            }
        }
    });
}

/// 右下纵向堆叠重排：最新触发贴底，其余按存活顺序向上错开。
fn relayout(app: &AppHandle) {
    let slots: Vec<(String, usize)> = {
        let reg = lock_registry();
        let count = reg.alive.len();
        reg.alive
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.label.clone(), count - 1 - index))
            .collect()
    };
    for (label, slot) in slots {
        if let Some(window) = app.get_webview_window(&label) {
            if let Some((x, y)) = popup_position(app, slot) {
                let _ = window.set_position(tauri::LogicalPosition::new(x, y));
            }
        }
    }
}

/// 弹窗窗口位置（逻辑像素）：主屏可见区域右下，第 `slot` 层向上堆叠（0 = 贴 Dock 边）。
fn popup_position(app: &AppHandle, slot: usize) -> Option<(f64, f64)> {
    let (work_x, work_y, work_w, work_h) = crate::work_area_logical(app)?;
    Some((
        work_x + work_w - POPUP_SIZE.0 - crate::EDGE_MARGIN,
        work_y + work_h
            - POPUP_SIZE.1
            - crate::EDGE_MARGIN
            - slot as f64 * (POPUP_SIZE.1 + POPUP_GAP),
    ))
}
