//! Store 单例、数据目录解析与外部变更监听。
//!
//! 数据目录解析与 CLI 严格对齐（两端必须读写同一目录）：`TEMPORULE_DATA_DIR` 环境变量
//! 优先，否则 `directories::ProjectDirs::from_path("com.temporule")`
//! （macOS `~/Library/Application Support/com.temporule`）。
//!
//! 外部变更监听：notify 目录监听 → 200ms 防抖 → 比对 `reminders.json` 内容哈希，
//! 变化才广播 [`STORE_CHANGED_EVENT`]；自写触发的监听由写入后同步哈希过滤掉，
//! 改由 [`reminders_written`] 直接向调用窗口以外的窗口广播（避免写入方重复重载）。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use temporule_core::store::{Store, REMINDERS_FILE};

// notify 的 `watch` 是 trait 方法，必须显式引入 trait
use notify::Watcher as _;

/// 「数据已变更」事件名：前端收到后重载提醒列表。
pub const STORE_CHANGED_EVENT: &str = "store-changed";

/// 目录监听防抖窗口：事件静默该时长后才判定一次变更。
const DEBOUNCE: Duration = Duration::from_millis(200);

/// 数据目录解析（与 CLI 一致）：`TEMPORULE_DATA_DIR` 优先，否则 `com.temporule` 项目目录。
pub fn data_dir() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("TEMPORULE_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    directories::ProjectDirs::from_path(PathBuf::from("com.temporule"))
        .map(|dirs| dirs.data_dir().to_path_buf())
        .ok_or_else(|| "无法解析应用数据目录".to_string())
}

/// 全局 Store 单例（惰性初始化；初始化失败在每次调用上报错）。
static STORE: LazyLock<Result<Mutex<Store>, String>> = LazyLock::new(|| {
    let dir = data_dir()?;
    let store =
        Store::open(&dir).map_err(|err| format!("无法打开数据目录 {}：{err}", dir.display()))?;
    Ok(Mutex::new(store))
});

/// 取全局 Store 单例（互斥访问，命令并发安全）。
pub fn store() -> Result<MutexGuard<'static, Store>, String> {
    match &*STORE {
        Ok(mutex) => mutex.lock().map_err(|_| "存储锁已中毒".to_string()),
        Err(err) => Err(err.clone()),
    }
}

/// `reminders.json` 内容哈希的最近已见值；自写与外部写共用同一过滤。
static LAST_HASH: Mutex<Option<u64>> = Mutex::new(None);

/// 仪表盘「失焦自动隐藏」开关的运行期缓存（失焦回调里不做 IO）。
static DASHBOARD_HIDE_ON_BLUR: AtomicBool = AtomicBool::new(true);

/// 同步「失焦自动隐藏」缓存（启动读设置与保存设置时调用）。
pub fn set_dashboard_hide_on_blur(hide: bool) {
    DASHBOARD_HIDE_ON_BLUR.store(hide, Ordering::Relaxed);
}

/// 读取「失焦自动隐藏」缓存。
pub fn dashboard_hide_on_blur() -> bool {
    DASHBOARD_HIDE_ON_BLUR.load(Ordering::Relaxed)
}

/// 启动时把磁盘设置同步到运行期缓存。
pub fn init_settings_cache() -> Result<(), String> {
    let settings = store()?.load_settings().map_err(|err| err.to_string())?;
    set_dashboard_hide_on_blur(settings.dashboard_hide_on_blur);
    Ok(())
}

/// 写入提醒数据后同步哈希，让自写触发的目录监听被过滤掉。
pub fn note_reminders_written() {
    set_last_hash(reminders_hash());
}

/// 提醒数据写入后的统一收尾：登记自写哈希（目录监听不重复广播），并把
/// [`STORE_CHANGED_EVENT`] 广播给调用窗口以外的全部窗口——调用窗口由自身收尾流程
/// 重建列表（api.ts `onStoreChanged` 的「外部修改提醒数据」契约）。
pub fn reminders_written(window: &WebviewWindow) {
    note_reminders_written();
    let app = window.app_handle();
    for (label, other) in app.webview_windows() {
        if label != window.label() {
            let _ = other.emit(STORE_CHANGED_EVENT, ());
        }
    }
}

/// 计算 `reminders.json` 当前内容哈希；文件缺失返回 `None`。
fn reminders_hash() -> Option<u64> {
    let bytes = std::fs::read(data_dir().ok()?.join(REMINDERS_FILE)).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(hasher.finish())
}

/// 覆写最近已见哈希。
fn set_last_hash(hash: Option<u64>) {
    if let Ok(mut last) = LAST_HASH.lock() {
        *last = hash;
    }
}

/// 与最近已见值比对并登记；不一致返回 `true`（视为真实变更）。
fn hash_changed(hash: u64) -> bool {
    match LAST_HASH.lock() {
        Ok(mut last) => {
            if *last == Some(hash) {
                false
            } else {
                *last = Some(hash);
                true
            }
        }
        Err(_) => false,
    }
}

/// 目录监听句柄保活槽。
static WATCHER: Mutex<Option<notify::RecommendedWatcher>> = Mutex::new(None);

/// 启动数据目录监听：事件防抖 200ms → 比对 `reminders.json` 内容哈希 → 变化才广播。
pub fn setup_watcher(app: &AppHandle) -> Result<(), String> {
    // 先触碰 Store 单例（确保数据目录存在），并登记当前哈希，
    // 避免把启动前的既有数据当作外部变更广播
    drop(store()?);
    note_reminders_written();

    let dir = data_dir()?;
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        let _ = tx.send(result);
    })
    .map_err(|err| format!("无法创建数据目录监听：{err}"))?;
    watcher
        .watch(&dir, notify::RecursiveMode::NonRecursive)
        .map_err(|err| format!("无法监听数据目录 {}：{err}", dir.display()))?;
    // 监听器保活到进程结束
    if let Ok(mut slot) = WATCHER.lock() {
        *slot = Some(watcher);
    }
    let handle = app.clone();
    // 启动前已存在的 invoke 请求（CLI 在 App 未运行时调用 open）：
    // 目录监听只报增量，启动前的存量请求在此直接消费
    consume_invoke(app);
    std::thread::spawn(move || watch_loop(handle, rx));
    Ok(())
}

/// 防抖消费循环：突发事件（原子写的 tmp + rename）合并为一次判定。
fn watch_loop(app: AppHandle, rx: Receiver<notify::Result<notify::Event>>) {
    while rx.recv().is_ok() {
        while rx.recv_timeout(DEBOUNCE).is_ok() {}
        consume_invoke(&app);
        if let Some(hash) = reminders_hash() {
            if hash_changed(hash) {
                let _ = app.emit(STORE_CHANGED_EVENT, ());
            }
        }
    }
}

/// 处理 CLI → GUI 唤起请求（`invoke.json`，consume-once）。
pub fn consume_invoke(app: &AppHandle) {
    let action = match store().map(|s| s.take_invoke()) {
        Ok(Ok(Some(action))) => action,
        Ok(Ok(None)) => return,
        Ok(Err(err)) => {
            eprintln!("[temporule] 读取 invoke 请求失败: {err}");
            return;
        }
        Err(err) => {
            eprintln!("[temporule] 打开 store 失败: {err}");
            return;
        }
    };
    let result = match action.as_str() {
        "show_dashboard" => crate::show_dashboard(app),
        "show_main" => crate::show_main_window(app),
        other => {
            eprintln!("[temporule] 未知 invoke 动作: {other}");
            return;
        }
    };
    if let Err(err) = result {
        eprintln!("[temporule] 处理 invoke 动作失败: {err}");
    }
}
