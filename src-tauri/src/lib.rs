pub mod runtime;

use kvmddc_core::{config::AppConfig, db::SwitchSource, deskflow::load_or_generate_tls_identity};
use runtime::{
    default_data_dir, AppRuntime, DashboardStateDto, EdgeCandidateDto, EdgePickerSignal, HudSignal,
};
use std::str::FromStr;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
#[cfg(target_os = "macos")]
use tauri::{menu::Submenu, RunEvent};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    DragDropEvent, LogicalPosition, Manager, PhysicalPosition, PhysicalSize, Position, Rect, Size,
    State, Theme, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

static LAST_TRAY_BLUR_MS: AtomicU64 = AtomicU64::new(0);
static LAST_TRAY_CLICK_MS: AtomicU64 = AtomicU64::new(0);
static HUD_HIDE_SEQ: AtomicU64 = AtomicU64::new(0);

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct ManagedRuntime(Arc<AppRuntime>);

#[tauri::command]
async fn get_dashboard_state(rt: State<'_, ManagedRuntime>) -> Result<DashboardStateDto, String> {
    rt.0.dashboard_snapshot().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn switch_to_host(
    rt: State<'_, ManagedRuntime>,
    host_id: String,
    source: Option<String>,
) -> Result<DashboardStateDto, String> {
    let src = source
        .as_deref()
        .and_then(|s| SwitchSource::from_str(s).ok())
        .unwrap_or(SwitchSource::Menu);
    let _ = rt
        .0
        .orchestrate_switch(&host_id, src)
        .await
        .map_err(|e| e.to_string())?;
    rt.0.dashboard_snapshot().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn wake_and_switch_host(
    rt: State<'_, ManagedRuntime>,
    host_id: String,
) -> Result<DashboardStateDto, String> {
    let _ = rt
        .0
        .orchestrate_switch(&host_id, SwitchSource::Menu)
        .await
        .map_err(|e| e.to_string())?;
    rt.0.dashboard_snapshot().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn leave_desk_to_windows(
    rt: State<'_, ManagedRuntime>,
) -> Result<DashboardStateDto, String> {
    let _ = rt
        .0
        .orchestrate_switch("win-desktop", SwitchSource::DisconnectGuard)
        .await
        .map_err(|e| e.to_string())?;
    rt.0.dashboard_snapshot().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_brightness(
    rt: State<'_, ManagedRuntime>,
    brightness: u8,
) -> Result<DashboardStateDto, String> {
    rt.0.set_brightness_global(brightness)
        .await
        .map_err(|e| e.to_string())?;
    rt.0.dashboard_snapshot().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_volume(
    rt: State<'_, ManagedRuntime>,
    volume: u8,
) -> Result<DashboardStateDto, String> {
    rt.0.set_volume_global(Some(volume), None)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn toggle_audio_mute(
    rt: State<'_, ManagedRuntime>,
    muted: bool,
) -> Result<DashboardStateDto, String> {
    rt.0.set_volume_global(None, Some(muted))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cycle_color_preset(rt: State<'_, ManagedRuntime>) -> Result<DashboardStateDto, String> {
    rt.0.cycle_color_preset().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_color_preset(
    rt: State<'_, ManagedRuntime>,
    preset: u8,
) -> Result<DashboardStateDto, String> {
    rt.0.select_color_preset(preset)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_display_mode(
    rt: State<'_, ManagedRuntime>,
    host_id: Option<String>,
    mode_id: Option<i32>,
    width: u32,
    height: u32,
    hz: u32,
    hidpi: bool,
) -> Result<DashboardStateDto, String> {
    rt.0.select_display_mode(host_id, mode_id.unwrap_or(-1), width, height, hz, hidpi)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_input_provider(
    rt: State<'_, ManagedRuntime>,
    host_id: String,
) -> Result<DashboardStateDto, String> {
    rt.0.set_input_provider(host_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_deskflow_enabled(
    rt: State<'_, ManagedRuntime>,
    enabled: bool,
) -> Result<DashboardStateDto, String> {
    rt.0.set_deskflow_enabled(enabled)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn toggle_ctrl_edge_switch(
    rt: State<'_, ManagedRuntime>,
) -> Result<DashboardStateDto, String> {
    rt.0.toggle_ctrl_edge_switch()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn toggle_cross_os_modifiers(
    rt: State<'_, ManagedRuntime>,
) -> Result<DashboardStateDto, String> {
    rt.0.toggle_cross_os_modifiers()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn send_files_to_host(
    rt: State<'_, ManagedRuntime>,
    target_host_id: String,
    paths: Vec<String>,
) -> Result<DashboardStateDto, String> {
    rt.0.send_paths_to_host(target_host_id, paths)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_internal_alignment(
    rt: State<'_, ManagedRuntime>,
    percent: u8,
) -> Result<DashboardStateDto, String> {
    rt.0.set_internal_alignment(percent)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn sleep_displays(rt: State<'_, ManagedRuntime>) -> Result<DashboardStateDto, String> {
    rt.0.sleep_displays_all().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn wake_displays(rt: State<'_, ManagedRuntime>) -> Result<DashboardStateDto, String> {
    rt.0.wake_displays_all().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn swap_input_provider(rt: State<'_, ManagedRuntime>) -> Result<DashboardStateDto, String> {
    rt.0.swap_input_provider().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn toggle_plain_slide(rt: State<'_, ManagedRuntime>) -> Result<DashboardStateDto, String> {
    rt.0.toggle_plain_slide().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn pair_peers_now(rt: State<'_, ManagedRuntime>) -> Result<DashboardStateDto, String> {
    rt.0.pair_peers_now().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn gather_windows() -> Result<(), String> {
    tokio::task::spawn_blocking(kvmddc_platform::gather_external_windows_to_builtin)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_app_theme(app: tauri::AppHandle, theme: String) {
    let target = if theme == "light" {
        Theme::Light
    } else {
        Theme::Dark
    };
    let theme_js = if theme == "light" { "light" } else { "dark" };
    for label in ["main", "tray", "picker", "hud"] {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_theme(Some(target));
            if label != "main" {
                let _ = w.eval(&format!(
                    "window.__applyThemeFromHost && window.__applyThemeFromHost('{theme_js}')"
                ));
            }
        }
    }
}

#[tauri::command]
async fn set_host_port_override(
    rt: State<'_, ManagedRuntime>,
    host_id: String,
    monitor_input: Option<u8>,
) -> Result<DashboardStateDto, String> {
    rt.0.set_host_port_override(host_id, monitor_input)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn open_accessibility_settings(
    rt: State<'_, ManagedRuntime>,
) -> Result<DashboardStateDto, String> {
    rt.0.open_accessibility_settings()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn relaunch_app(
    app: tauri::AppHandle,
    rt: State<'_, ManagedRuntime>,
) -> Result<Option<DashboardStateDto>, String> {
    // First check if macOS already updated TCC trust in-process — if so, activate immediately without restarting!
    if rt.0.refresh_accessibility_trust().await {
        let dto = rt.0.dashboard_snapshot().await.map_err(|e| e.to_string())?;
        return Ok(Some(dto));
    }

    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                "sleep 0.45 && /usr/bin/open -a /Applications/Kvmddc.app --args --open",
            ])
            .spawn();
        app.exit(0);
    }
    #[cfg(not(target_os = "macos"))]
    {
        app.restart();
    }
    #[allow(unreachable_code)]
    Ok(None)
}

#[tauri::command]
async fn dismiss_alert(rt: State<'_, ManagedRuntime>) -> Result<DashboardStateDto, String> {
    rt.0.dismiss_last_error().await.map_err(|e| e.to_string())
}

#[tauri::command]
fn open_main_window(app: tauri::AppHandle) {
    if let Some(tw) = app.get_webview_window("tray") {
        let _ = tw.eval("window.__closeTrayTransientMenus && window.__closeTrayTransientMenus()");
        let _ = tw.hide();
    }
    show_main_window(&app);
}

#[tauri::command]
fn close_tray_popover(app: tauri::AppHandle) {
    if let Some(tw) = app.get_webview_window("tray") {
        let _ = tw.eval("window.__closeTrayTransientMenus && window.__closeTrayTransientMenus()");
        let _ = tw.hide();
    }
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
async fn get_edge_candidates(
    rt: State<'_, ManagedRuntime>,
) -> Result<Vec<EdgeCandidateDto>, String> {
    Ok(rt.0.get_edge_candidates().await)
}

#[tauri::command]
async fn hover_edge_candidate(
    rt: State<'_, ManagedRuntime>,
    host_id: String,
) -> Result<(), String> {
    rt.0.hover_edge_candidate(host_id).await;
    Ok(())
}

#[tauri::command]
async fn confirm_edge_pick(
    rt: State<'_, ManagedRuntime>,
    host_id: String,
) -> Result<DashboardStateDto, String> {
    rt.0.confirm_edge_pick(host_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cancel_edge_pick(rt: State<'_, ManagedRuntime>) -> Result<(), String> {
    rt.0.hide_edge_picker().await;
    Ok(())
}

fn sd_round_rect(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hx - r);
    let qy = (py - cy).abs() - (hy - r);
    let out = (qx.max(0.0).hypot(qy.max(0.0))) + qx.max(qy).min(0.0);
    out - r
}

fn sd_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let pax = px - ax;
    let pay = py - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let denom = (bax * bax + bay * bay).max(1e-5);
    let h = ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0);
    (pax - bax * h).hypot(pay - bay * h)
}

/// Exact 40x36 (@2x of 20x18pt — matching macOS NSStatusItem's 18pt height 1:1 without resampling blur)
/// 4x4-supersampled SF-Symbol style `display.2` template icon.
fn build_tray_template_icon() -> Image<'static> {
    let w = 40usize;
    let h = 36usize;
    let mut rgba = vec![0u8; w * h * 4];
    let fg: [u8; 3] = if cfg!(target_os = "windows") {
        [238, 242, 248]
    } else {
        [0, 0, 0] // macOS template icon uses alpha mask
    };

    for y in 0..h {
        for x in 0..w {
            let mut cov = 0.0f32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = x as f32 + (sx as f32 + 0.5) * 0.25;
                    let py = y as f32 + (sy as f32 + 0.5) * 0.25;

                    // 1. Primary 27" Studio Display (upper-left)
                    let d_mon_ring =
                        sd_round_rect(px, py, 15.6, 13.0, 12.0, 8.2, 3.0).abs() - 1.45;
                    let d_stand_neck = sd_segment(px, py, 15.6, 21.3, 15.6, 26.8) - 1.35;
                    let d_stand_foot = sd_segment(px, py, 10.0, 27.2, 19.0, 27.2) - 1.35;
                    let d_back = d_mon_ring.min(d_stand_neck).min(d_stand_foot);

                    // Clean cutout halo around foreground 14" MacBook screen
                    let d_cutout = sd_round_rect(px, py, 27.8, 23.8, 9.8, 7.6, 3.4);
                    let back_ink = d_back <= 0.0 && d_cutout > 0.0;

                    // 2. Secondary 14" MacBook Display (lower-right overlap)
                    let d_mbp_ring =
                        sd_round_rect(px, py, 27.8, 23.0, 7.4, 5.0, 2.2).abs() - 1.38;
                    let d_mbp_deck = sd_segment(px, py, 18.8, 30.4, 37.0, 30.4) - 1.38;
                    let front_ink = d_mbp_ring.min(d_mbp_deck) <= 0.0;

                    if back_ink || front_ink {
                        cov += 1.0 / 16.0;
                    }
                }
            }

            let alpha = (cov * 255.0).round() as u8;
            if alpha > 0 {
                let idx = (y * w + x) * 4;
                rgba[idx] = fg[0];
                rgba[idx + 1] = fg[1];
                rgba[idx + 2] = fg[2];
                rgba[idx + 3] = alpha;
            }
        }
    }

    Image::new_owned(rgba, w as u32, h as u32)
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg(target_os = "macos")]
fn elevate_macos_blanker_window(win: &tauri::WebviewWindow) {
    #[link(name = "objc")]
    extern "C" {
        fn sel_registerName(name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
        fn objc_msgSend();
    }
    if let Ok(ns_win) = win.ns_window() {
        if !ns_win.is_null() {
            unsafe {
                type MsgSendInt =
                    unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, isize);
                type MsgSendUint =
                    unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, usize);
                let send_int: MsgSendInt = std::mem::transmute(objc_msgSend as *const ());
                let send_uint: MsgSendUint = std::mem::transmute(objc_msgSend as *const ());
                // NSScreenSaverWindowLevel (1000) + 1 = 1001: covers Menu Bar & Dock without Screen Lock
                let sel_level = sel_registerName(b"setLevel:\0".as_ptr() as *const _);
                send_int(ns_win, sel_level, 1001);
                // CanJoinAllSpaces | Stationary | IgnoresCycle | FullScreenAuxiliary
                let sel_beh = sel_registerName(b"setCollectionBehavior:\0".as_ptr() as *const _);
                send_uint(ns_win, sel_beh, (1 << 0) | (1 << 4) | (1 << 6) | (1 << 8));
            }
        }
    }
}

fn sync_standby_blankers(app: &tauri::AppHandle, active: bool) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if !active {
            for idx in 0..8 {
                let label = format!("blanker-{idx}");
                if let Some(w) = handle.get_webview_window(&label) {
                    let _ = w.set_cursor_visible(true);
                    let _ = w.hide();
                }
            }
            return;
        }

        if let Some(tw) = handle.get_webview_window("tray") {
            let _ = tw.hide();
        }

        let mut monitors = handle.available_monitors().unwrap_or_default();
        if monitors.is_empty() {
            if let Ok(Some(pm)) = handle.primary_monitor() {
                monitors.push(pm);
            }
        }

        let count = monitors.len().min(8);
        for (idx, mon) in monitors.into_iter().take(8).enumerate() {
            let label = format!("blanker-{idx}");
            let pos = *mon.position();
            let sz = *mon.size();
            let scale = mon.scale_factor().max(1.0);
            let lw = (sz.width as f64 / scale).max(100.0);
            let lh = (sz.height as f64 / scale).max(100.0);
            let lx = pos.x as f64 / scale;
            let ly = pos.y as f64 / scale;

            let win_opt = if let Some(existing) = handle.get_webview_window(&label) {
                Some(existing)
            } else {
                WebviewWindowBuilder::new(
                    &handle,
                    &label,
                    WebviewUrl::App("blanker.html".into()),
                )
                .title("Kvmddc Standby")
                .position(lx, ly)
                .inner_size(lw, lh)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .shadow(false)
                .focused(idx == 0)
                .visible(false)
                .build()
                .ok()
            };

            if let Some(w) = win_opt {
                let _ = w.set_position(Position::Physical(PhysicalPosition::new(pos.x, pos.y)));
                let _ = w.set_size(Size::Physical(PhysicalSize::new(sz.width, sz.height)));
                let _ = w.set_always_on_top(true);
                let _ = w.set_visible_on_all_workspaces(true);
                #[cfg(target_os = "macos")]
                elevate_macos_blanker_window(&w);
                let _ = w.eval("window.__armBlanker && window.__armBlanker()");
                let _ = w.show();
                let _ = w.set_cursor_visible(false);
                if idx == 0 {
                    let _ = w.set_focus();
                }
            }
        }

        for extra in count..8 {
            let label = format!("blanker-{extra}");
            if let Some(w) = handle.get_webview_window(&label) {
                let _ = w.set_cursor_visible(true);
                let _ = w.hide();
            }
        }
    });
}

fn show_hud_overlay(app: &tauri::AppHandle, sig: HudSignal) {
    let seq = HUD_HIDE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    let handle = app.clone();
    let payload_json = serde_json::to_string(&sig).unwrap_or_else(|_| "{}".to_string());

    let _ = app.run_on_main_thread(move || {
        let Some(hw) = handle.get_webview_window("hud") else {
            return;
        };
        let _ = hw.set_ignore_cursor_events(true);
        let _ = hw.set_always_on_top(true);
        let _ = hw.set_visible_on_all_workspaces(true);
        let scale = hw.scale_factor().unwrap_or(2.0).max(1.0);
        let hud_w = (304.0 * scale).round() as i32;
        if let Ok(Some(mon)) = handle.primary_monitor() {
            let mpos = mon.position();
            let msize = mon.size();
            let tx = mpos.x + ((msize.width as i32 - hud_w) / 2).max(16);
            let ty = mpos.y + (44.0 * scale).round() as i32;
            let _ = hw.set_position(Position::Physical(PhysicalPosition::new(tx, ty)));
        }
        let _ = hw.eval(&format!(
            "window.__renderHudSignal && window.__renderHudSignal({payload_json})"
        ));
        let _ = hw.show();
    });

    let hide_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1350)).await;
        if HUD_HIDE_SEQ.load(Ordering::Relaxed) == seq {
            if let Some(hw) = hide_handle.get_webview_window("hud") {
                let _ = hw.hide();
            }
        }
    });
}

fn toggle_tray_popover(app: &tauri::AppHandle, rect: Rect, click_pos: PhysicalPosition<f64>) {
    LAST_TRAY_CLICK_MS.store(now_epoch_ms(), Ordering::Relaxed);

    let Some(tw) = app.get_webview_window("tray") else {
        show_main_window(app);
        return;
    };

    if tw.is_visible().unwrap_or(false) {
        let _ = tw.eval("window.__closeTrayTransientMenus && window.__closeTrayTransientMenus()");
        let _ = tw.hide();
        return;
    }

    let since_blur = now_epoch_ms().saturating_sub(LAST_TRAY_BLUR_MS.load(Ordering::Relaxed));
    if since_blur < 220 {
        return;
    }

    #[cfg(target_os = "macos")]
    if let Some(sample) = kvmddc_platform::sample_pointer_and_ctrl() {
        let pop_w = 332.0_f64;
        let min_x = sample.screen_left as f64 + 10.0;
        let max_x = (sample.screen_right as f64 - pop_w - 10.0).max(min_x);
        let target_lx = (sample.x as f64 - pop_w * 0.5).clamp(min_x, max_x);
        let menu_bottom = (sample.y as f64 + 12.0)
            .max(sample.screen_top as f64 + 28.0)
            .min(sample.screen_top as f64 + 44.0);
        let target_ly = menu_bottom + 6.0;

        let _ = tw.set_position(Position::Logical(LogicalPosition::new(
            target_lx.round(),
            target_ly.round(),
        )));
        let _ = tw.eval("window.__refreshTrayPopover && window.__refreshTrayPopover()");
        let _ = tw.show();
        let _ = tw.set_focus();
        return;
    }

    let target_mon = tw
        .available_monitors()
        .ok()
        .and_then(|mons| {
            let cx = click_pos.x.round() as i32;
            let cy = click_pos.y.round() as i32;
            mons.into_iter().find(|m| {
                let p = m.position();
                let s = m.size();
                cx >= p.x && cx < p.x + s.width as i32 && cy >= p.y && cy < p.y + s.height as i32
            })
        })
        .or_else(|| tw.current_monitor().ok().flatten());

    let scale = target_mon
        .as_ref()
        .map(|m| m.scale_factor())
        .or_else(|| tw.scale_factor().ok())
        .unwrap_or(2.0);
    let pop_w = (332.0 * scale).round() as i32;
    let pop_h = (640.0 * scale).round() as i32;

    let (rx, ry) = match rect.position {
        Position::Physical(p) => (p.x as f64, p.y as f64),
        Position::Logical(l) => (l.x * scale, l.y * scale),
    };
    let (rw, rh) = match rect.size {
        Size::Physical(s) => (s.width as f64, s.height as f64),
        Size::Logical(s) => (s.width * scale, s.height * scale),
    };

    let anchor_cx = if rw > 1.0 {
        rx + rw * 0.5
    } else {
        click_pos.x
    };
    let anchor_bottom = if rh > 1.0 {
        ry + rh
    } else {
        click_pos.y + 12.0 * scale
    };

    let mut target_x = (anchor_cx - (pop_w as f64) * 0.5).round() as i32;
    let mut target_y = (anchor_bottom + (6.0 * scale)).round() as i32;

    if let Some(mon) = target_mon {
        let mpos = mon.position();
        let msize = mon.size();
        let margin = (10.0 * scale).round() as i32;
        let min_x = mpos.x + margin;
        let max_x = (mpos.x + msize.width as i32 - pop_w - margin).max(min_x);
        target_x = target_x.clamp(min_x, max_x);

        let mid_y = mpos.y + (msize.height as i32) / 2;
        if (ry as i32) > mid_y {
            target_y = (ry as i32 - pop_h - (8.0 * scale).round() as i32).max(mpos.y + margin);
        }
    }

    let _ = tw.set_position(Position::Physical(PhysicalPosition::new(target_x, target_y)));
    let _ = tw.eval("window.__refreshTrayPopover && window.__refreshTrayPopover()");
    let _ = tw.show();
    let _ = tw.set_focus();
}

pub fn run_entry() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let data_dir = default_data_dir();

    if let Some(idx) = args.iter().position(|a| a == "--init-config") {
        let host_id = args
            .get(idx + 1)
            .ok_or_else(|| anyhow::anyhow!("缺少 <local_host_id> 参数"))?;
        let token = args
            .get(idx + 2)
            .ok_or_else(|| anyhow::anyhow!("缺少 <shared_token> 参数"))?;
        std::fs::create_dir_all(&data_dir)?;
        let identity = load_or_generate_tls_identity(&data_dir, host_id)?;
        let mut cfg = AppConfig::default_home_setup(host_id, token.clone());
        for h in &mut cfg.hosts {
            if h.id == *host_id {
                h.tls_fingerprint = Some(identity.v2_fingerprint.clone());
            }
        }
        cfg.save(&data_dir)?;
        let out = serde_json::json!({
            "host_id": host_id,
            "data_dir": data_dir,
            "tls_fingerprint": identity.v2_fingerprint,
        });
        println!("{}", serde_json::to_string(&out)?);
        return Ok(());
    }

    kvmddc_platform::kill_other_kvmddc_instances();

    if args.iter().any(|a| a == "--headless") {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        rt.block_on(async move {
            let app_rt = AppRuntime::initialize(data_dir)?;
            app_rt.spawn_background_loops();
            tokio::signal::ctrl_c().await?;
            Ok::<(), anyhow::Error>(())
        })?;
        return Ok(());
    }

    let launch_in_background = args.iter().any(|a| a == "--background")
        && !args.iter().any(|a| a == "--open");

    let app = tauri::Builder::default()
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            {
                // First-class macOS application: visible in Cmd+Tab, Dock, and native App Menu
                app.set_activation_policy(tauri::ActivationPolicy::Regular);

                let app_sub = Submenu::with_items(
                    app,
                    "Kvmddc",
                    true,
                    &[
                        &PredefinedMenuItem::about(app, Some("关于 Kvmddc"), None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::hide(app, Some("隐藏 Kvmddc"))?,
                        &PredefinedMenuItem::hide_others(app, Some("隐藏其他"))?,
                        &PredefinedMenuItem::show_all(app, Some("全部显示"))?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::quit(app, Some("退出 Kvmddc"))?,
                    ],
                )?;
                let show_stage_item = MenuItem::with_id(
                    app,
                    "app_open_stage",
                    "打开主舞台",
                    true,
                    Some("CmdOrCtrl+1"),
                )?;
                let win_sub = Submenu::with_items(
                    app,
                    "窗口",
                    true,
                    &[
                        &show_stage_item,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::minimize(app, Some("最小化"))?,
                        &PredefinedMenuItem::close_window(app, Some("关闭窗口"))?,
                    ],
                )?;
                let app_menu = Menu::with_items(app, &[&app_sub, &win_sub])?;
                let _ = app.set_menu(app_menu);
                app.on_menu_event(|app_handle, ev| {
                    if ev.id().as_ref() == "app_open_stage" {
                        show_main_window(app_handle);
                    }
                });
            }

            let app_rt = AppRuntime::initialize(data_dir.clone())?;
            let handle_for_picker = app.handle().clone();
            let handle_for_blanker = app.handle().clone();
            let handle_for_hud = app.handle().clone();
            let rt_for_cb = Arc::clone(&app_rt);
            tauri::async_runtime::spawn(async move {
                rt_for_cb
                    .set_picker_callback(Arc::new(move |sig| {
                        if let Some(pw) = handle_for_picker.get_webview_window("picker") {
                            match sig {
                                EdgePickerSignal::Show { x, y } => {
                                    let _ = pw.set_position(Position::Physical(
                                        PhysicalPosition::new(x, y),
                                    ));
                                    let _ = pw.eval(
                                        "window.__reloadPickerCandidates && window.__reloadPickerCandidates()",
                                    );
                                    let _ = pw.show();
                                }
                                EdgePickerSignal::Hide => {
                                    let _ = pw.hide();
                                }
                            }
                        }
                    }))
                    .await;
                rt_for_cb
                    .set_blanker_callback(Arc::new(move |active| {
                        sync_standby_blankers(&handle_for_blanker, active);
                    }))
                    .await;
                rt_for_cb
                    .set_hud_callback(Arc::new(move |sig| {
                        show_hud_overlay(&handle_for_hud, sig);
                    }))
                    .await;
                rt_for_cb.spawn_background_loops();
            });

            app.manage(ManagedRuntime(Arc::clone(&app_rt)));

            let m_open = MenuItem::with_id(app, "open", "打开 Kvmddc 主舞台", true, None::<&str>)?;
            let m_sleep = MenuItem::with_id(app, "sleep_disp", "熄屏待机", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let m_quit = MenuItem::with_id(app, "quit", "退出 Kvmddc", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&m_open, &m_sleep, &sep, &m_quit])?;

            let rt_menu = Arc::clone(&app_rt);
            TrayIconBuilder::new()
                .icon(build_tray_template_icon())
                .icon_as_template(cfg!(target_os = "macos"))
                .tooltip("Kvmddc · 三机切屏与显示控制")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        position,
                        rect,
                        ..
                    } = event
                    {
                        toggle_tray_popover(tray.app_handle(), rect, position);
                    }
                })
                .on_menu_event(move |app_handle, event| {
                    let id = event.id().as_ref().to_string();
                    let rt_c = Arc::clone(&rt_menu);
                    match id.as_str() {
                        "open" => show_main_window(app_handle),
                        "sleep_disp" => {
                            tauri::async_runtime::spawn(async move {
                                let _ = rt_c.sleep_displays_all().await;
                            });
                        }
                        "quit" => {
                            app_handle.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            let needs_mac_perm =
                cfg!(target_os = "macos") && !kvmddc_platform::is_accessibility_trusted();
            if (!launch_in_background || needs_mac_perm) && !cfg!(target_os = "windows") {
                show_main_window(app.handle());
            }

            Ok(())
        })
        .on_window_event(|window, event| match window.label() {
            "main" => match event {
                WindowEvent::Focused(false) => {
                    if let Some(wv) = window.app_handle().get_webview_window("main") {
                        let _ = wv.eval(
                            "window.__closeStageTransientMenus && window.__closeStageTransientMenus()",
                        );
                    }
                }
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    if let Some(wv) = window.app_handle().get_webview_window("main") {
                        let _ = wv.eval(
                            "window.__closeStageTransientMenus && window.__closeStageTransientMenus()",
                        );
                    }
                    let _ = window.hide();
                }
                WindowEvent::DragDrop(dd) => {
                    let Some(wv) = window.app_handle().get_webview_window("main") else {
                        return;
                    };
                    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
                    match dd {
                        DragDropEvent::Enter { position, .. }
                        | DragDropEvent::Over { position, .. } => {
                            let lx = position.x / scale;
                            let ly = position.y / scale;
                            let _ = wv.eval(&format!(
                                "window.__onNativeFileDragOver && window.__onNativeFileDragOver({lx:.1},{ly:.1})"
                            ));
                        }
                        DragDropEvent::Leave => {
                            let _ = wv.eval(
                                "window.__onNativeFileDragLeave && window.__onNativeFileDragLeave()",
                            );
                        }
                        DragDropEvent::Drop { paths, position } => {
                            let lx = position.x / scale;
                            let ly = position.y / scale;
                            let list: Vec<String> = paths
                                .iter()
                                .map(|p| p.to_string_lossy().to_string())
                                .collect();
                            if let Ok(json_paths) = serde_json::to_string(&list) {
                                let _ = wv.eval(&format!(
                                    "window.__onNativeFileDrop && window.__onNativeFileDrop({lx:.1},{ly:.1},{json_paths})"
                                ));
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            },
            "tray" => match event {
                WindowEvent::Focused(false) => {
                    LAST_TRAY_BLUR_MS.store(now_epoch_ms(), Ordering::Relaxed);
                    if let Some(tw) = window.app_handle().get_webview_window("tray") {
                        let _ = tw.eval(
                            "window.__closeTrayTransientMenus && window.__closeTrayTransientMenus()",
                        );
                    }
                    let _ = window.hide();
                }
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    if let Some(tw) = window.app_handle().get_webview_window("tray") {
                        let _ = tw.eval(
                            "window.__closeTrayTransientMenus && window.__closeTrayTransientMenus()",
                        );
                    }
                    let _ = window.hide();
                }
                _ => {}
            },
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_dashboard_state,
            switch_to_host,
            wake_and_switch_host,
            leave_desk_to_windows,
            set_brightness,
            set_volume,
            toggle_audio_mute,
            cycle_color_preset,
            set_color_preset,
            set_display_mode,
            set_input_provider,
            set_deskflow_enabled,
            toggle_ctrl_edge_switch,
            toggle_cross_os_modifiers,
            send_files_to_host,
            set_internal_alignment,
            sleep_displays,
            wake_displays,
            swap_input_provider,
            toggle_plain_slide,
            pair_peers_now,
            gather_windows,
            set_app_theme,
            set_host_port_override,
            open_accessibility_settings,
            relaunch_app,
            dismiss_alert,
            open_main_window,
            close_tray_popover,
            quit_app,
            get_edge_candidates,
            hover_edge_candidate,
            confirm_edge_pick,
            cancel_edge_pick,
        ])
        .build(tauri::generate_context!())
        .map_err(|e| anyhow::anyhow!("Tauri 初始化失败: {e}"))?;

    app.run(|app_handle, event| {
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen { .. } = event {
            let since_tray =
                now_epoch_ms().saturating_sub(LAST_TRAY_CLICK_MS.load(Ordering::Relaxed));
            if since_tray > 400 {
                show_main_window(app_handle);
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (app_handle, event);
    });

    Ok(())
}
