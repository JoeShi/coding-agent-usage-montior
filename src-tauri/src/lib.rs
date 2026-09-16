//! Tauri application entry: tray, scheduler, IPC commands.

pub mod config;
pub mod credentials;
pub mod log;
pub mod model;
pub mod providers;
pub mod volc_sigv4;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use model::{DataSource, SourceStatus, UsageSnapshot};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

// ---------------------------------------------------------------- state

pub struct AppState {
    pub snapshots: Mutex<HashMap<DataSource, UsageSnapshot>>,
    pub config: Mutex<config::AppConfig>,
    pub refresh_tx: tokio::sync::mpsc::Sender<()>,
}

impl AppState {
    fn latest(&self) -> Vec<UsageSnapshot> {
        self.snapshots.lock().unwrap().values().cloned().collect()
    }
}

// ---------------------------------------------------------------- tray icon

/// Whether any window of any source is at/over the acceleration threshold.
fn over_threshold(snapshots: &HashMap<DataSource, UsageSnapshot>) -> bool {
    snapshots.values().any(|s| {
        s.status == SourceStatus::Ok
            && s.windows
                .iter()
                .any(|w| w.ratio() >= config::ACCELERATION_THRESHOLD)
    })
}

/// 16x16 RGBA dot icon; red when over threshold, gray otherwise.
fn dot_icon(warn: bool) -> tauri::image::Image<'static> {
    let (r, g, b) = if warn {
        (0xe1u8, 0x4du8, 0x42u8)
    } else {
        (0x99u8, 0x99u8, 0x99u8)
    };
    let size = 16usize;
    let mut rgba = vec![0u8; size * size * 4];
    let center = (size as f64 - 1.0) / 2.0;
    let radius = 6.0f64;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f64 - center;
            let dy = y as f64 - center;
            if dx * dx + dy * dy <= radius * radius {
                let i = (y * size + x) * 4;
                rgba[i] = r;
                rgba[i + 1] = g;
                rgba[i + 2] = b;
                rgba[i + 3] = 0xff;
            }
        }
    }
    tauri::image::Image::new_owned(rgba, size as u32, size as u32)
}

fn update_tray(app: &AppHandle) {
    let state = app.state::<AppState>();
    let warn = over_threshold(&state.snapshots.lock().unwrap());
    if let Some(tray) = app.tray_by_id("main") {
        // Icon-only tray: the dot carries the whole signal (red = over
        // threshold); per-source percentages live in the detail panel.
        let _ = tray.set_title(None::<&str>);
        let _ = tray.set_icon(Some(dot_icon(warn)));
    }
}

// ---------------------------------------------------------------- refresh

async fn refresh_all(app: &AppHandle) {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap_or_default();
    let (kimi, ark, kiro, codex) = tokio::join!(
        providers::kimi::fetch_usage(&http),
        providers::ark::fetch_afp_usage(&http),
        providers::kiro::fetch_usage(&http),
        providers::codex::fetch_usage()
    );
    {
        let state = app.state::<AppState>();
        let mut snapshots = state.snapshots.lock().unwrap();
        merge_snapshot(&mut snapshots, kimi);
        merge_snapshot(&mut snapshots, ark);
        merge_snapshot(&mut snapshots, kiro);
        merge_snapshot(&mut snapshots, codex);
    }
    update_tray(app);
    let state = app.state::<AppState>();
    let _ = app.emit("snapshots-updated", state.latest());
}

/// Keep previous window data when the new fetch is Stale (spec: failed
/// refreshes preserve last-good data).
fn merge_snapshot(map: &mut HashMap<DataSource, UsageSnapshot>, new: UsageSnapshot) {
    if new.status == SourceStatus::Stale {
        if let Some(old) = map.get(&new.source) {
            if matches!(old.status, SourceStatus::Ok | SourceStatus::Stale)
                && !old.windows.is_empty()
            {
                let mut merged = old.clone();
                merged.status = SourceStatus::Stale;
                if new.extras != Default::default() {
                    merged.extras = new.extras.clone();
                }
                merged.message = new.message.clone();
                map.insert(new.source, merged);
                return;
            }
        }
    }
    map.insert(new.source, new);
}

/// Polling interval given current config + data: accelerated when any
/// window is >= 80%, otherwise the configured base (clamped to >= 30s).
fn poll_interval(
    cfg: &config::AppConfig,
    snapshots: &HashMap<DataSource, UsageSnapshot>,
) -> Duration {
    if over_threshold(snapshots) {
        Duration::from_secs(config::ACCELERATED_POLL_INTERVAL_SECS)
    } else {
        Duration::from_secs(cfg.poll_interval_secs.max(30))
    }
}

/// Polling loop: base interval from config, accelerated when any window
/// is >= 80%, manual refresh via channel.
async fn scheduler_loop(app: AppHandle, mut refresh_rx: tokio::sync::mpsc::Receiver<()>) {
    loop {
        let interval = {
            let state = app.state::<AppState>();
            let cfg = state.config.lock().unwrap().clone();
            let snaps = state.snapshots.lock().unwrap().clone();
            poll_interval(&cfg, &snaps)
        };
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = refresh_rx.recv() => {}
        }
        refresh_all(&app).await;
    }
}

// ---------------------------------------------------------------- commands

#[tauri::command]
fn get_snapshots(app: AppHandle) -> Vec<UsageSnapshot> {
    app.state::<AppState>().latest()
}

#[tauri::command]
fn get_config(app: AppHandle) -> config::AppConfig {
    app.state::<AppState>().config.lock().unwrap().clone()
}

#[tauri::command]
fn save_config(app: AppHandle, config: config::AppConfig) -> Result<(), String> {
    config::save(&config)?;
    *app.state::<AppState>().config.lock().unwrap() = config;
    update_tray(&app);
    Ok(())
}

#[tauri::command]
async fn refresh_now(app: AppHandle) -> Result<(), String> {
    let _ = app.state::<AppState>().refresh_tx.try_send(());
    Ok(())
}

// ---------------------------------------------------------------- kiro

#[derive(Serialize)]
struct KiroCredStatus {
    configured: bool,
}

#[tauri::command]
fn get_kiro_cred_status() -> KiroCredStatus {
    KiroCredStatus {
        configured: credentials::get_kiro_api_key().is_some(),
    }
}

/// Validate the key with a live GetUsageLimits call; only persist on success.
#[tauri::command]
async fn save_kiro_credentials(app: AppHandle, key: String) -> Result<UsageSnapshot, String> {
    let key = key.trim().to_string();
    if !key.starts_with("ksk_") {
        return Err("Kiro API Key 应以 ksk_ 开头".into());
    }
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let snap = providers::kiro::fetch_with_key(&http, &key).await;
    match snap.status {
        SourceStatus::Ok => {
            credentials::save_kiro_api_key(&key)?;
            let _ = app.state::<AppState>().refresh_tx.try_send(());
            Ok(snap)
        }
        SourceStatus::AuthError => Err("验证失败：API Key 无效或已被服务端拒绝（403）".into()),
        _ => Err("验证失败：网络错误或服务不可用，Key 未保存".into()),
    }
}

#[tauri::command]
fn clear_kiro_credentials(app: AppHandle) -> Result<(), String> {
    credentials::clear_kiro_api_key()?;
    let _ = app.state::<AppState>().refresh_tx.try_send(());
    Ok(())
}

#[tauri::command]
async fn get_ark_usage_details(
    start: String,
    end: String,
    interval: String,
) -> Result<Vec<providers::ark::UsageDetail>, String> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    providers::ark::fetch_usage_details(&http, &start, &end, &interval)
        .await
        .map_err(|e| match e {
            providers::ark::FetchError::Status(s) => format!("credential state: {s:?}"),
            providers::ark::FetchError::Transient(m) => m,
        })
}

// ---------------------------------------------------------------- setup

/// Position the popover under the menu bar on the display the user is
/// interacting with, using logical (point) coordinates.
///
/// Why not tauri-plugin-positioner's TrayCenter: tao's `set_outer_position`
/// converts physical positions with the window's *current* screen scale
/// factor. When the window last sat on a display with a different DPI than
/// the one being clicked (mixed-DPI multi-monitor), the conversion is wrong
/// and the window lands off-screen; the visible toggle then appears dead
/// (tauri-apps/tauri#7890 family). Logical coordinates skip that conversion
/// entirely.
///
/// `tray_rect` is the clicked tray icon's rect (physical) when invoked from
/// a tray click; menu-item invocations pass None and fall back to cursor.
///
/// Coordinate space notes (macOS, mixed-DPI):
/// - `cursor_position()` returns "physical" = global points x the PRIMARY
///   display's scale, while `monitor_from_point` compares against
///   `CGDisplayBounds` rects, which are in global POINTS. Dividing the cursor
///   by the primary scale converts back to points before hit-testing —
///   passing physical coords directly misses every CGDisplayBounds rect and
///   silently falls back to the wrong monitor.
/// - Tray rect / monitor position+size are physical with each screen's OWN
///   scale, so divide those by the target monitor's scale.
fn position_popover(win: &tauri::WebviewWindow, tray_rect: Option<(f64, f64, f64, f64)>) {
    let app = win.app_handle().clone();
    let primary_scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    // Global points, top-left origin (CGDisplayBounds space).
    let cursor_pt = win
        .cursor_position()
        .ok()
        .map(|p| (p.x / primary_scale, p.y / primary_scale));
    let monitor = cursor_pt
        .and_then(|(cx, cy)| app.monitor_from_point(cx, cy).ok().flatten())
        .or_else(|| win.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let scale = monitor.scale_factor();
    let m_pos = monitor.position();
    let m_size = monitor.size();
    let (mx, my) = (m_pos.x as f64 / scale, m_pos.y as f64 / scale);
    let mw = m_size.width as f64 / scale;
    // The window's size in points is screen-independent.
    let win_scale = win.scale_factor().unwrap_or(scale);
    let ww = win
        .outer_size()
        .map(|s| s.width as f64 / win_scale)
        .unwrap_or(360.0);
    let (anchor_x, y) = match tray_rect {
        // Click path: center on the icon, hug its bottom edge.
        Some((tx, ty, tw, th)) => ((tx + tw / 2.0) / scale, (ty + th) / scale + 2.0),
        // Menu path: the cursor is over the open menu below the menu bar;
        // center on it horizontally and anchor just below the menu bar.
        None => (
            cursor_pt.map(|(cx, _)| cx).unwrap_or(mx + mw / 2.0),
            my + 24.0,
        ),
    };
    let x = (anchor_x - ww / 2.0).clamp(mx, mx + mw - ww);
    crate::log::log(&format!(
        "popover positioned at ({x:.0},{y:.0}) on monitor ({mx:.0},{my:.0} {mw:.0}w, scale {scale})"
    ));
    let _ = win.set_position(tauri::LogicalPosition::new(x, y));
}

fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        position_popover(&win, None);
        let _ = win.show();
        let _ = win.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (refresh_tx, refresh_rx) = tokio::sync::mpsc::channel::<()>(4);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_positioner::init())
        .manage(AppState {
            snapshots: Mutex::new(HashMap::new()),
            config: Mutex::new(config::load()),
            refresh_tx,
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshots,
            get_config,
            save_config,
            refresh_now,
            get_ark_usage_details,
            get_kiro_cred_status,
            save_kiro_credentials,
            clear_kiro_credentials,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // Popover 风格：毛玻璃背景（随系统深浅色自适应）+ 原生圆角。
            #[cfg(target_os = "macos")]
            if let Some(win) = app.get_webview_window("main") {
                use window_vibrancy::{
                    apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState,
                };
                // Menu 材质：与系统菜单栏下拉一致；Popover 会被壁纸染色（BehindWindow 混合）。
                if let Err(e) = apply_vibrancy(
                    &win,
                    NSVisualEffectMaterial::Menu,
                    Some(NSVisualEffectState::Active),
                    Some(12.0),
                ) {
                    eprintln!("apply_vibrancy failed: {e}");
                }
            }

            // Tray: left click toggles the detail window, menu for actions.
            let refresh_i = MenuItem::with_id(app, "refresh", "立即刷新", true, None::<&str>)?;
            let open_i = MenuItem::with_id(app, "open", "打开面板", true, None::<&str>)?;
            let settings_i = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&refresh_i, &open_i, &settings_i, &quit_i])?;

            TrayIconBuilder::with_id("main")
                .icon(dot_icon(false))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "refresh" => {
                        let _ = app.state::<AppState>().refresh_tx.try_send(());
                    }
                    "open" => show_main_window(app),
                    "settings" => {
                        show_main_window(app);
                        let _ = app.emit("navigate", "settings");
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    // 让 positioner 记录托盘图标位置（保留插件状态；定位本身
                    // 由 position_popover 用逻辑坐标完成，见该函数注释）。
                    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            if win.is_visible().unwrap_or(false) {
                                let _ = win.hide();
                            } else {
                                let _ = app.state::<AppState>().refresh_tx.try_send(());
                                let r = (
                                    rect.position.to_physical::<f64>(1.0).x,
                                    rect.position.to_physical::<f64>(1.0).y,
                                    rect.size.to_physical::<f64>(1.0).width,
                                    rect.size.to_physical::<f64>(1.0).height,
                                );
                                position_popover(&win, Some(r));
                                let _ = win.show();
                                let _ = win.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // Kick off initial fetch + scheduler.
            tauri::async_runtime::spawn(async move {
                refresh_all(&handle).await;
                scheduler_loop(handle.clone(), refresh_rx).await;
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            // Hide instead of closing: menu-bar app stays resident.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
            // Popover 行为：点击面板之外（失焦）自动收起。
            if let tauri::WindowEvent::Focused(false) = event {
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {});
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::QuotaWindow;

    fn snap(source: DataSource, status: SourceStatus, used: f64, quota: f64) -> UsageSnapshot {
        let mut s = UsageSnapshot::new(source, status);
        if quota > 0.0 {
            s.windows.push(QuotaWindow {
                label: "5h".into(),
                used,
                quota,
                reset_at: None,
            });
        }
        s
    }

    #[test]
    fn merge_keeps_last_good_on_repeated_stale_fetches() {
        let mut map = HashMap::new();
        let mut initial = snap(DataSource::KimiCode, SourceStatus::Ok, 10.0, 100.0);
        initial.extras.plan_tier = Some("pro".into());
        map.insert(DataSource::KimiCode, initial);

        let mut first_stale = snap(DataSource::KimiCode, SourceStatus::Stale, 0.0, 0.0);
        first_stale.message = Some("temporary failure".into());
        merge_snapshot(&mut map, first_stale);
        merge_snapshot(
            &mut map,
            snap(DataSource::KimiCode, SourceStatus::Stale, 0.0, 0.0),
        );

        let snapshot = &map[&DataSource::KimiCode];
        assert_eq!(snapshot.status, SourceStatus::Stale, "marked stale");
        assert_eq!(snapshot.windows[0].used, 10.0, "keeps last-good data");
        assert_eq!(
            snapshot.extras.plan_tier.as_deref(),
            Some("pro"),
            "keeps last-good extras when stale fetch has none"
        );
    }

    #[test]
    fn merge_replaces_on_ok_and_hard_errors() {
        let mut map = HashMap::new();
        map.insert(
            DataSource::KimiCode,
            snap(DataSource::KimiCode, SourceStatus::Stale, 5.0, 100.0),
        );
        merge_snapshot(
            &mut map,
            snap(DataSource::KimiCode, SourceStatus::Ok, 6.0, 100.0),
        );
        assert_eq!(map[&DataSource::KimiCode].status, SourceStatus::Ok);
        assert_eq!(map[&DataSource::KimiCode].windows[0].used, 6.0);

        merge_snapshot(
            &mut map,
            snap(DataSource::KimiCode, SourceStatus::NeedRelogin, 0.0, 0.0),
        );
        assert_eq!(map[&DataSource::KimiCode].status, SourceStatus::NeedRelogin);
        assert!(map[&DataSource::KimiCode].windows.is_empty());
    }

    #[test]
    fn acceleration_threshold_boundary() {
        let mut map = HashMap::new();
        map.insert(
            DataSource::KimiCode,
            snap(DataSource::KimiCode, SourceStatus::Ok, 79.0, 100.0),
        );
        assert!(!over_threshold(&map));
        map.insert(
            DataSource::KimiCode,
            snap(DataSource::KimiCode, SourceStatus::Ok, 80.0, 100.0),
        );
        assert!(over_threshold(&map));
    }

    #[test]
    fn acceleration_applies_to_kiro_monthly_window() {
        let mut map = HashMap::new();
        map.insert(
            DataSource::KiroCli,
            snap(DataSource::KiroCli, SourceStatus::Ok, 85.0, 100.0),
        );
        assert!(over_threshold(&map));
    }

    #[test]
    fn acceleration_applies_to_codex_window() {
        let mut map = HashMap::new();
        map.insert(
            DataSource::Codex,
            snap(DataSource::Codex, SourceStatus::Ok, 80.0, 100.0),
        );
        assert!(over_threshold(&map));
    }

    #[test]
    fn interval_switches_on_threshold() {
        let mut cfg = config::AppConfig::default();
        cfg.poll_interval_secs = 300;
        let mut map = HashMap::new();
        map.insert(
            DataSource::KimiCode,
            snap(DataSource::KimiCode, SourceStatus::Ok, 10.0, 100.0),
        );
        assert_eq!(poll_interval(&cfg, &map), Duration::from_secs(300));

        map.insert(
            DataSource::KimiCode,
            snap(DataSource::KimiCode, SourceStatus::Ok, 90.0, 100.0),
        );
        assert_eq!(
            poll_interval(&cfg, &map),
            Duration::from_secs(config::ACCELERATED_POLL_INTERVAL_SECS)
        );

        // Clamp pathological config values.
        cfg.poll_interval_secs = 1;
        map.insert(
            DataSource::KimiCode,
            snap(DataSource::KimiCode, SourceStatus::Ok, 0.0, 100.0),
        );
        assert_eq!(poll_interval(&cfg, &map), Duration::from_secs(30));
    }
}
