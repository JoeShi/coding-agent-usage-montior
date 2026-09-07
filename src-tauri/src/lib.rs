//! Tauri application entry: tray, scheduler, IPC commands.

pub mod config;
pub mod credentials;
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
use tauri_plugin_positioner::{Position, WindowExt};

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
            && s.windows.iter().any(|w| w.ratio() >= config::ACCELERATION_THRESHOLD)
    })
}

/// 16x16 RGBA dot icon; red when over threshold, gray otherwise.
fn dot_icon(warn: bool) -> tauri::image::Image<'static> {
    let (r, g, b) = if warn { (0xe1u8, 0x4du8, 0x42u8) } else { (0x99u8, 0x99u8, 0x99u8) };
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
    let (kimi, ark, kiro) = tokio::join!(
        providers::kimi::fetch_usage(&http),
        providers::ark::fetch_afp_usage(&http),
        providers::kiro::fetch_usage(&http)
    );
    {
        let state = app.state::<AppState>();
        let mut snapshots = state.snapshots.lock().unwrap();
        merge_snapshot(&mut snapshots, kimi);
        merge_snapshot(&mut snapshots, ark);
        merge_snapshot(&mut snapshots, kiro);
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
            if old.status == SourceStatus::Ok {
                let mut merged = old.clone();
                merged.status = SourceStatus::Stale;
                // Keep extras from the new fetch (e.g. plan_tier).
                merged.extras = new.extras.clone();
                map.insert(new.source, merged);
                return;
            }
        }
    }
    map.insert(new.source, new);
}

/// Polling interval given current config + data: accelerated when any
/// window is >= 80%, otherwise the configured base (clamped to >= 30s).
fn poll_interval(cfg: &config::AppConfig, snapshots: &HashMap<DataSource, UsageSnapshot>) -> Duration {
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

fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.move_window(Position::TrayCenter);
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
                use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
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
                    // 让 positioner 记录托盘图标位置，TrayCenter 定位依赖它。
                    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            if win.is_visible().unwrap_or(false) {
                                let _ = win.hide();
                            } else {
                                let _ = app.state::<AppState>().refresh_tx.try_send(());
                                let _ = win.move_window(Position::TrayCenter);
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
    fn merge_keeps_last_good_on_stale() {
        let mut map = HashMap::new();
        map.insert(DataSource::KimiCode, snap(DataSource::KimiCode, SourceStatus::Ok, 10.0, 100.0));
        merge_snapshot(&mut map, snap(DataSource::KimiCode, SourceStatus::Stale, 0.0, 0.0));
        let s = &map[&DataSource::KimiCode];
        assert_eq!(s.status, SourceStatus::Stale, "marked stale");
        assert_eq!(s.windows[0].used, 10.0, "but keeps last-good data");
    }

    #[test]
    fn merge_replaces_on_ok_and_hard_errors() {
        let mut map = HashMap::new();
        map.insert(DataSource::KimiCode, snap(DataSource::KimiCode, SourceStatus::Stale, 5.0, 100.0));
        merge_snapshot(&mut map, snap(DataSource::KimiCode, SourceStatus::Ok, 6.0, 100.0));
        assert_eq!(map[&DataSource::KimiCode].status, SourceStatus::Ok);
        assert_eq!(map[&DataSource::KimiCode].windows[0].used, 6.0);

        merge_snapshot(&mut map, snap(DataSource::KimiCode, SourceStatus::NeedRelogin, 0.0, 0.0));
        assert_eq!(map[&DataSource::KimiCode].status, SourceStatus::NeedRelogin);
        assert!(map[&DataSource::KimiCode].windows.is_empty());
    }

    #[test]
    fn acceleration_threshold_boundary() {
        let mut map = HashMap::new();
        map.insert(DataSource::KimiCode, snap(DataSource::KimiCode, SourceStatus::Ok, 79.0, 100.0));
        assert!(!over_threshold(&map));
        map.insert(DataSource::KimiCode, snap(DataSource::KimiCode, SourceStatus::Ok, 80.0, 100.0));
        assert!(over_threshold(&map));
    }

    #[test]
    fn acceleration_applies_to_kiro_monthly_window() {
        let mut map = HashMap::new();
        map.insert(DataSource::KiroCli, snap(DataSource::KiroCli, SourceStatus::Ok, 85.0, 100.0));
        assert!(over_threshold(&map));
    }

    #[test]
    fn interval_switches_on_threshold() {
        let mut cfg = config::AppConfig::default();
        cfg.poll_interval_secs = 300;
        let mut map = HashMap::new();
        map.insert(DataSource::KimiCode, snap(DataSource::KimiCode, SourceStatus::Ok, 10.0, 100.0));
        assert_eq!(poll_interval(&cfg, &map), Duration::from_secs(300));

        map.insert(DataSource::KimiCode, snap(DataSource::KimiCode, SourceStatus::Ok, 90.0, 100.0));
        assert_eq!(
            poll_interval(&cfg, &map),
            Duration::from_secs(config::ACCELERATED_POLL_INTERVAL_SECS)
        );

        // Clamp pathological config values.
        cfg.poll_interval_secs = 1;
        map.insert(DataSource::KimiCode, snap(DataSource::KimiCode, SourceStatus::Ok, 0.0, 100.0));
        assert_eq!(poll_interval(&cfg, &map), Duration::from_secs(30));
    }
}
