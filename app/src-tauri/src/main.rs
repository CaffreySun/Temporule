//! Temporule（拾律）Tauri 2 GUI 入口。
//!
//! 切片 3 骨架：托盘图标、无 Dock 图标（activation policy Accessory）、
//! 惰性窗口（仪表盘 / 主窗口）、deep link（`temporule://dashboard`）、单实例；
//! 切片 4 接入 temporule-core 数据命令（`commands`）与 Store 单例/目录监听（`store_state`）；
//! 调度循环与弹窗窗口在切片 5 接入。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod popup;
mod scheduler;
mod store_state;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use url::Url;

/// 仪表盘窗口标签（前端按 label 分发视图）。
const DASHBOARD_LABEL: &str = "dashboard";

/// 主窗口标签（前端按 label 分发视图）。
const MAIN_LABEL: &str = "main";

/// deep link 协议名（不含 `://`）。
const DEEP_LINK_SCHEME: &str = "temporule";

/// deep link 仪表盘路由的 host 部分（`temporule://dashboard`）。
const DEEP_LINK_DASHBOARD_HOST: &str = "dashboard";

/// 托盘图标 id。
const TRAY_ID: &str = "temporule-tray";

/// 托盘菜单项 id：显示仪表盘。
const MENU_DASHBOARD_ID: &str = "dashboard";

/// 托盘菜单项 id：显示主窗口。
const MENU_MAIN_ID: &str = "main";

/// 托盘菜单项 id：退出应用。
const MENU_QUIT_ID: &str = "quit";

/// 托盘图标（64px，构建期嵌入）。
const TRAY_ICON_BYTES: &[u8] = include_bytes!("../icons/tray.png");

/// 仪表盘窗口尺寸（逻辑像素）。
const DASHBOARD_SIZE: (f64, f64) = (440.0, 560.0);

/// 主窗口尺寸（逻辑像素）。
const MAIN_SIZE: (f64, f64) = (860.0, 620.0);

/// 窗口与屏幕可见区域（右缘 / Dock 上缘）的间距（逻辑像素）。
pub(crate) const EDGE_MARGIN: f64 = 16.0;

/// 主屏可见区域（逻辑像素）：x、y、宽、高；取不到主屏时返回 `None`（由系统默认定位兜底）。
pub(crate) fn work_area_logical(app: &AppHandle) -> Option<(f64, f64, f64, f64)> {
    let monitor = app.primary_monitor().ok().flatten()?;
    let scale = monitor.scale_factor();
    let work_area = monitor.work_area();
    Some((
        work_area.position.x as f64 / scale,
        work_area.position.y as f64 / scale,
        work_area.size.width as f64 / scale,
        work_area.size.height as f64 / scale,
    ))
}

/// 计算仪表盘窗口位置：对齐托盘图标（菜单栏里它在哪，仪表盘就在哪）。
///
/// 用托盘图标 rect（物理像素、左上原点，与 `builder.position` 同坐标系）：
/// 水平居中对齐托盘图标、垂直落在其下方 8px；x 在所属屏幕内夹取，防止贴边溢出。
/// 取不到托盘 rect 时退化为主屏右上（菜单栏下方）。
fn dashboard_position(app: &AppHandle) -> Option<(f64, f64)> {
    let tray_rect = app
        .tray_by_id(TRAY_ID)
        .and_then(|t| t.rect().ok().flatten())?;
    // rect 返回物理像素（左上原点，与 builder.position 同坐标系）
    let (tx, ty, tw, th) = match (tray_rect.position, tray_rect.size) {
        // 物理分支为整数像素，逻辑分支为浮点——统一转 f64
        (tauri::Position::Physical(p), tauri::Size::Physical(s)) => (
            f64::from(p.x),
            f64::from(p.y),
            f64::from(s.width),
            f64::from(s.height),
        ),
        (tauri::Position::Logical(p), tauri::Size::Logical(s)) => (p.x, p.y, s.width, s.height),
        // 混合逻辑/物理组合不预期，放弃自定义定位
        _ => return None,
    };
    let tray_cx = tx + tw / 2.0;
    let mut x = tray_cx - DASHBOARD_SIZE.0 / 2.0;
    let mut y = ty + th + 8.0;
    // 在托盘所属屏幕内夹取
    if let Ok(monitors) = app.available_monitors() {
        if let Some(m) = monitors.iter().find(|m| {
            let mx = m.position().x as f64;
            let mw = m.size().width as f64;
            tray_cx >= mx && tray_cx <= mx + mw
        }) {
            let mx = m.position().x as f64;
            let my = m.position().y as f64;
            let mw = m.size().width as f64;
            let mh = m.size().height as f64;
            x = x.clamp(mx + EDGE_MARGIN, mx + mw - DASHBOARD_SIZE.0 - EDGE_MARGIN);
            y = y.min(my + mh - DASHBOARD_SIZE.1 - EDGE_MARGIN);
        }
    }
    Some((x, y))
}

/// 按「主窗口是否可见」切换激活策略。
///
/// macOS 原生语义：主窗口 = 常规应用（Dock/Cmd-Tab 可见）→ Regular；
/// 仅有仪表盘/弹窗时 = 菜单栏面板（无 Dock 图标）→ Accessory。
pub(crate) fn update_activation_policy(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let main_visible = app
            .get_webview_window(MAIN_LABEL)
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false);
        let policy = if main_visible {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        let _ = app.set_activation_policy(policy);
    }
}

/// 显示并聚焦仪表盘窗口；不存在时惰性创建。
///
/// 仪表盘是菜单栏面板：常驻 Accessory（无 Dock 图标）；若主窗口开着则维持 Regular。
/// `set_focus` 内部已带 `activateIgnoringOtherApps: YES`（tao 实现），窗口照常置前。
fn show_dashboard(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    update_activation_policy(app);
    if let Some(window) = app.get_webview_window(DASHBOARD_LABEL) {
        window.show()?;
        window.set_focus()?;
        return Ok(window);
    }
    let mut builder = WebviewWindowBuilder::new(
        app,
        DASHBOARD_LABEL,
        WebviewUrl::App("index.html?view=dashboard".into()),
    )
    .title("拾律")
    .inner_size(DASHBOARD_SIZE.0, DASHBOARD_SIZE.1)
    .always_on_top(true)
    .skip_taskbar(true)
    .closable(true)
    .minimizable(false)
    // 悬浮面板无系统标题栏：标题区自绘（前端 header 可拖动 + 关闭按钮）
    // transparent：frameless 窗口 macOS 不自动圆角，靠透明窗口 + CSS 圆角实现
    .decorations(false)
    .transparent(true);
    if let Some((x, y)) = dashboard_position(app) {
        builder = builder.position(x, y);
    }
    let window = builder.build()?;
    // 失焦自动隐藏（设置页「仪表盘失焦隐藏」开关控制，默认开）
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(false) = event {
            if store_state::dashboard_hide_on_blur() {
                if let Some(window) = handle.get_webview_window(DASHBOARD_LABEL) {
                    let _ = window.hide();
                }
            }
        }
    });
    window.show()?;
    window.set_focus()?;
    Ok(window)
}

/// 显示并聚焦主窗口；不存在时惰性创建（启动默认隐藏，仅经托盘 / invoke 唤起）。
///
/// 主窗口 = 常规应用：显示前切 Regular（Dock/Cmd-Tab 出现）；关闭后回 Accessory。
pub(crate) fn show_main_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window(MAIN_LABEL) {
        window.show()?;
        window.set_focus()?;
        return Ok(window);
    }
    let window = WebviewWindowBuilder::new(
        app,
        MAIN_LABEL,
        WebviewUrl::App("index.html?view=main".into()),
    )
    .title("拾律")
    .inner_size(MAIN_SIZE.0, MAIN_SIZE.1)
    // v3：Overlay 透明标题栏——红绿灯浮在深色内容上方，标题栏与内容同为 --surface，零割裂
    .title_bar_style(tauri::TitleBarStyle::Overlay)
    .transparent(true)
    .hidden_title(true)
    .build()?;
    let handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(
            event,
            tauri::WindowEvent::Destroyed | tauri::WindowEvent::CloseRequested { .. }
        ) {
            update_activation_policy(&handle);
        }
    });
    window.show()?;
    window.set_focus()?;
    Ok(window)
}

/// 处理 deep link URL：`temporule://dashboard` → 仪表盘；`temporule://main` → 主窗口。
///
/// 其余 URL 静默忽略。
fn handle_deep_link(app: &AppHandle, raw: &str) {
    let Ok(url) = Url::parse(raw) else {
        return;
    };
    if url.scheme() != DEEP_LINK_SCHEME {
        return;
    }
    let result = match url.host_str() {
        Some(DEEP_LINK_DASHBOARD_HOST) => show_dashboard(app),
        Some("main") => show_main_window(app),
        _ => return,
    };
    if let Err(err) = result {
        eprintln!("[temporule] deep link 窗口显示失败: {err}");
    }
}

/// 构建托盘图标与右键菜单。
///
/// 左键点击（`show_menu_on_left_click(false)`）显示/聚焦仪表盘；
/// 右键菜单：仪表盘 / 主窗口 / 退出拾律。
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let dashboard_item = MenuItem::with_id(app, MENU_DASHBOARD_ID, "仪表盘", true, None::<&str>)?;
    let main_item = MenuItem::with_id(app, MENU_MAIN_ID, "主窗口", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, MENU_QUIT_ID, "退出拾律", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&dashboard_item, &main_item, &quit_item])?;
    let icon = tauri::image::Image::from_bytes(TRAY_ICON_BYTES)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("拾律")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_DASHBOARD_ID => {
                if let Err(err) = show_dashboard(app) {
                    eprintln!("[temporule] 显示仪表盘失败: {err}");
                }
            }
            MENU_MAIN_ID => {
                if let Err(err) = show_main_window(app) {
                    eprintln!("[temporule] 显示主窗口失败: {err}");
                }
            }
            MENU_QUIT_ID => {
                // 退出清理的弹窗窗口不产生 dismissed / 递补
                popup::mark_shutting_down();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Err(err) = show_dashboard(tray.app_handle()) {
                    eprintln!("[temporule] 显示仪表盘失败: {err}");
                }
            }
        })
        .build(app)?;
    Ok(())
}

fn main() {
    let app = tauri::Builder::default()
        // 单实例：二次启动只把参数透传给首实例，不新建进程窗口
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            for arg in &argv {
                handle_deep_link(app, arg);
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::AppleScript,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            commands::list,
            commands::get,
            commands::add,
            commands::update,
            commands::remove,
            commands::toggle,
            commands::history,
            commands::settings,
            commands::save_settings,
            commands::popup_action,
            commands::popup_detail,
            commands::show_main_window,
            commands::data_dir,
            commands::open_data_dir,
            commands::store_rev,
            commands::request_notification_permission,
            commands::app_info,
            commands::open_external
        ])
        .setup(|app| {
            // 菜单栏常驻：无 Dock / Cmd-Tab 图标（macOS 专有 API，见 CONVENTIONS §1 跨平台就绪规范）
            #[cfg(target_os = "macos")]
            app.handle()
                .set_activation_policy(tauri::ActivationPolicy::Accessory)?;

            setup_tray(app)?;

            // Store 单例 + 设置缓存 + 数据目录监听（外部修改 → store-changed 事件）
            store_state::init_settings_cache()?;
            store_state::setup_watcher(app.handle())?;

            // 调度循环：启动错过扫描 + 过期 snooze 清扫，随后每分钟对齐边界 tick
            scheduler::start(app.handle().clone());

            // deep link：macOS 方案注册在构建期写入 Info.plist，运行期只订阅打开事件
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let handle = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    for url in event.urls() {
                        handle_deep_link(&handle, url.as_str());
                    }
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("temporule 应用启动失败");
    app.run(|_app, event| {
        // 菜单栏常驻：最后一个窗口（含弹窗）关闭不退出应用；
        // 托盘「退出拾律」走 app.exit（code = Some）不在此拦截
        if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
