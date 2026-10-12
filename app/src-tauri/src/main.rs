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

/// 计算仪表盘窗口位置：主屏可见区域右下角，距 Dock 与右缘 16px。
///
/// 返回逻辑像素坐标；取不到主屏时返回 `None`（由系统默认定位兜底）。
fn dashboard_position(app: &AppHandle) -> Option<(f64, f64)> {
    let (work_x, work_y, work_w, work_h) = work_area_logical(app)?;
    Some((
        work_x + work_w - DASHBOARD_SIZE.0 - EDGE_MARGIN,
        work_y + work_h - DASHBOARD_SIZE.1 - EDGE_MARGIN,
    ))
}

/// 按「是否有可见前端窗口」切换激活策略。
///
/// 菜单栏常驻（Accessory）下非活跃 App 的窗口不会被置前/显示；
/// 显示窗口前需切 Regular，全部隐藏后回 Accessory（无 Dock 图标）。
pub(crate) fn update_activation_policy(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let any_visible = [DASHBOARD_LABEL, MAIN_LABEL, "popup-"]
            .iter()
            .any(|prefix| {
                app.webview_windows()
                    .values()
                    .any(|w| w.label().starts_with(prefix) && w.is_visible().unwrap_or(false))
            });
        let policy = if any_visible {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        let _ = app.set_activation_policy(policy);
    }
}

/// 显示并聚焦仪表盘窗口；不存在时惰性创建（创建时定位屏幕右下）。
fn show_dashboard(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    // 显示前先切 Regular（此时窗口尚未可见，按可见性计算会误判为无窗口）
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
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
    .minimizable(false);
    if let Some((x, y)) = dashboard_position(app) {
        builder = builder.position(x, y);
    }
    let window = builder.build()?;
    // 失焦自动隐藏（设置页「仪表盘失焦隐藏」开关控制，默认开）
    let handle = app.clone();
    window.on_window_event(move |event| {
        match event {
            tauri::WindowEvent::Focused(false) => {
                if store_state::dashboard_hide_on_blur() {
                    if let Some(window) = handle.get_webview_window(DASHBOARD_LABEL) {
                        let _ = window.hide();
                        update_activation_policy(&handle);
                    }
                }
            }
            // 用户点 X 关闭仪表盘：按剩余可见窗口回切
            tauri::WindowEvent::Destroyed => update_activation_policy(&handle),
            _ => {}
        }
    });
    window.show()?;
    window.set_focus()?;
    Ok(window)
}

/// 显示并聚焦主窗口；不存在时惰性创建（启动默认隐藏，仅经托盘 / deep link 唤起）。
pub(crate) fn show_main_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    // 显示前先切 Regular（同 show_dashboard）
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
            commands::request_notification_permission
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
