use anyhow::{bail, ensure, Context, Result};
use axum::body::Bytes;
use chrono::{Local, Utc};
use kvmddc_core::{
    config::{
        color_preset_label, next_color_preset, validate_pure_lan_ip, AppConfig, HostNode,
        HostPlatform, SharedClusterSettings, SharedSyncOutcome,
    },
    db::{NewSwitchRequest, StateDb, SwitchRequestRow, SwitchSource, VerifyOutcome},
    deskflow::{
        create_isolated_short_tmpdir, load_or_generate_tls_identity, parse_deskflow_log_line,
        resolve_deskflow_binary, write_deskflow_runtime_files, DeskflowLogEvent, TlsIdentity,
    },
    protocol::{
        bind_pure_lan_listener, build_control_router, sha256_hex_bytes, ActionAck, BoxFuture,
        BrightnessRequest, ColorPresetRequest, ControlPlaneHandler, DisplayModeDto,
        DisplayModeRequest, FocusInputRequest, LanControlClient, PairPeerRequest,
        PairPeerResponse, PeerStatusResponse, RemoteSwitchRequest, VolumeRequest,
    },
    wol::send_wol_broadcast,
};
use kvmddc_platform::{
    default_port_label_for_input, list_external_input_devices, ConnectedMonitorProbe,
    DisplayModeOption, EdgeSide, ExternalInputDevice, MediaAdjustHotkey, StandbyWakeDetector,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{Notify, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardHostDto {
    pub id: String,
    pub display_name: String,
    pub platform: HostPlatform,
    pub lan_ip: String,
    pub monitor_input: u8,
    pub port_label: String,
    pub detected_live: bool,
    pub manual_port_override: bool,
    pub hotkey_slot: u8,
    pub enabled: bool,
    pub online: bool,
    pub profile_summary: String,
    pub live_mode_summary: Option<String>,
    pub internal_height_percent: u8,
    pub preferred_brightness: Option<u8>,
    pub preferred_color_preset: Option<u8>,
    pub color_preset_label: String,
    #[serde(default)]
    pub display_modes: Vec<DisplayModeDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardRequestDto {
    pub id: i64,
    pub to_host: Option<String>,
    pub target_input: u8,
    pub observed_input: Option<u8>,
    pub source: SwitchSource,
    pub status: String,
    pub attempts: u8,
    pub time_hms: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardStateDto {
    pub local_host_id: String,
    pub input_provider_host_id: String,
    pub primary_ddc_controller_id: String,
    pub deskflow_enabled: bool,
    pub ctrl_edge_switch_enabled: bool,
    pub allow_plain_slide_to_internal: bool,
    pub unify_cross_os_modifiers: bool,
    pub auto_switch_input_provider_on_hotplug: bool,
    pub observed_monitor_input: Option<u8>,
    pub preferred_brightness: u8,
    pub current_brightness: Option<u8>,
    pub preferred_volume: u8,
    pub current_volume: Option<u8>,
    pub audio_muted: bool,
    pub current_color_preset: u8,
    pub color_preset_label: String,
    pub last_transfer_summary: Option<String>,
    pub deskflow_running: bool,
    pub deskflow_connected: bool,
    pub accessibility_trusted: bool,
    pub display_sleeping: bool,
    pub external_display_online: bool,
    pub monitor_model: String,
    pub edge_locked: bool,
    #[serde(default)]
    pub clamshell_closed: bool,
    #[serde(default)]
    pub plain_slide_active: bool,
    pub secure_input_active: bool,
    pub cooldown_remaining_ms: u64,
    pub last_error: Option<String>,
    pub hosts: Vec<DashboardHostDto>,
    pub recent_requests: Vec<DashboardRequestDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeCandidateDto {
    pub id: String,
    pub display_name: String,
    pub monitor_input: u8,
    pub hotkey_slot: u8,
    pub online: bool,
}

#[derive(Debug, Clone)]
pub enum EdgePickerSignal {
    Show { x: i32, y: i32 },
    Hide,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HudSignal {
    pub kind: String,
    pub label: String,
    pub value: u8,
    pub muted: bool,
}

pub type EdgePickerCallback = Arc<dyn Fn(EdgePickerSignal) + Send + Sync>;
pub type StandbyBlankerCallback = Arc<dyn Fn(bool) + Send + Sync>;
pub type HudCallback = Arc<dyn Fn(HudSignal) + Send + Sync>;

struct LiveState {
    cfg: AppConfig,
    deskflow_running: bool,
    deskflow_connected: bool,
    accessibility_trusted: bool,
    display_sleeping: bool,
    saved_ddc_brightness_before_sleep: Option<u8>,
    external_display_online: bool,
    local_probe: ConnectedMonitorProbe,
    local_display_modes: Vec<DisplayModeDto>,
    peer_detected_inputs: HashMap<String, u8>,
    peer_detected_ports: HashMap<String, String>,
    peer_detected_modes: HashMap<String, String>,
    peer_display_modes: HashMap<String, Vec<DisplayModeDto>>,
    peer_monitor_model: Option<String>,
    active_cursor_host: Option<String>,
    deskflow_screen_host: Option<String>,
    observed_monitor_input: Option<u8>,
    current_brightness: Option<u8>,
    current_volume: Option<u8>,
    current_color_preset: Option<u8>,
    last_clipboard_png_sha: Option<String>,
    last_transfer_summary: Option<String>,
    edge_locked: bool,
    peer_edge_locked: HashMap<String, bool>,
    clamshell_closed: bool,
    peer_clamshell_closed: HashMap<String, bool>,
    applied_slide_peer_id: Option<String>,
    applied_mouse_adjustment: Option<String>,
    applied_confined_builtin: bool,
    last_switch_activity_at: Instant,
    secure_input_active: bool,
    last_error: Option<String>,
    peer_online: HashMap<String, bool>,
    picker_visible: bool,
    picker_bounds: Option<(i32, i32, i32, i32)>,
    edge_candidates: Vec<EdgeCandidateDto>,
    hovered_candidate_id: Option<String>,
}

static FOCUS_FOLLOWUP_SEQ: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
static FOCUS_FOLLOWUP_TARGET: std::sync::Mutex<Option<String>> =
    std::sync::Mutex::new(None);
static CLIENT_REFOCUS_SEQ: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
static PICKER_DISMISSED_SEQ: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
static LAST_PEER_SHAPE_CHANGE: std::sync::Mutex<Option<(String, Instant)>> =
    std::sync::Mutex::new(None);

fn sync_confine_builtin_flag(data_dir: &Path, confined: bool) {
    let df_dir = data_dir.join("deskflow");
    let flag_path = df_dir.join("confine_builtin.flag");
    if confined {
        let _ = std::fs::create_dir_all(&df_dir);
        let _ = std::fs::write(&flag_path, b"1");
    } else {
        let _ = std::fs::remove_file(&flag_path);
    }
}

/// Returns `true` when `probe` reports a normal desktop resolution (`>= 1280×720`) that the user
/// chose in OS Display Settings, rather than a broken VGA/WinDisc fallback (`640×480`..`1024×768`).
fn is_user_chosen_desktop_resolution(w: u32, h: u32) -> bool {
    w >= 1280 && h >= 720
}

impl LiveState {
    /// Returns the peer host ID mounted on the 27" shared monitor ONLY when plain screen-edge
    /// sliding (`27" monitor <-> laptop internal screen`) is physically valid:
    /// - `allow_plain_slide_to_internal` is enabled, displays are awake, and no remote session lock is active;
    /// - an enabled laptop host (`has_internal_screen == true`) exists and its lid is OPEN (`!clamshell_closed`);
    /// - the 27" shared monitor is currently displaying a DIFFERENT online host (`peer.id != internal_host.id`).
    /// Returns `true` when Deskflow on this host must pause because either this machine itself is
    /// screen/secure-input locked, or the active cluster `input_provider` is locked. An idle 3rd
    /// peer sitting at its lock screen must never freeze Deskflow between unlocked peers.
    fn is_deskflow_session_locked(&self) -> bool {
        self.edge_locked
            || (!self.cfg.is_input_provider()
                && self
                    .peer_edge_locked
                    .get(&self.cfg.input_provider_host_id)
                    .copied()
                    .unwrap_or(false))
    }

    fn effective_plain_slide_peer_id(&self) -> Option<String> {
        if !self.cfg.allow_plain_slide_to_internal
            || self.display_sleeping
            || self.is_deskflow_session_locked()
        {
            return None;
        }

        let enabled = self.cfg.enabled_hosts();
        let internal_host = enabled
            .iter()
            .copied()
            .find(|h| h.display_profile.has_internal_screen)?;

        let internal_locked = internal_host.id != self.cfg.local_host_id
            && self
                .peer_edge_locked
                .get(&internal_host.id)
                .copied()
                .unwrap_or(false);
        if internal_locked {
            return None;
        }

        let internal_lid_closed = if internal_host.id == self.cfg.local_host_id {
            self.clamshell_closed
        } else {
            if !self.peer_online.get(&internal_host.id).copied().unwrap_or(false) {
                return None;
            }
            self.peer_clamshell_closed
                .get(&internal_host.id)
                .copied()
                .unwrap_or(true)
        };
        if internal_lid_closed {
            return None;
        }

        let active_input = self.observed_monitor_input.or_else(|| {
            if self.external_display_online {
                self.local_probe.detected_input
            } else if self.peer_detected_inputs.len() == 1 {
                self.peer_detected_inputs.values().copied().next()
            } else {
                None
            }
        })?;

        let monitor_host = enabled
            .iter()
            .copied()
            .find(|h| h.monitor_input == active_input)?;

        if monitor_host.id == internal_host.id {
            return None;
        }

        let monitor_host_online = if monitor_host.id == self.cfg.local_host_id {
            true
        } else {
            self.peer_online
                .get(&monitor_host.id)
                .copied()
                .unwrap_or(false)
        };
        let monitor_host_locked = monitor_host.id != self.cfg.local_host_id
            && self
                .peer_edge_locked
                .get(&monitor_host.id)
                .copied()
                .unwrap_or(false);
        if !monitor_host_online || monitor_host_locked {
            return None;
        }

        Some(monitor_host.id.clone())
    }

    /// Returns `true` when this laptop's lid is open, its external cable still enumerates the 27" monitor
    /// as an extended desktop (`-X..0`), AND the 27" monitor is actually showing a different host.
    /// Confining Deskflow & pointer edge math to the built-in display avoids invisible `-X..0` cursor traps.
    fn should_confine_builtin_display(&self) -> bool {
        let Ok(local) = self.cfg.local_host() else {
            return false;
        };
        if !local.display_profile.has_internal_screen
            || self.clamshell_closed
            || !self.external_display_online
        {
            return false;
        }
        matches!(self.observed_monitor_input, Some(inp) if inp != local.monitor_input)
    }

    fn should_reload_deskflow_links(&self) -> bool {
        if !self.deskflow_running {
            return false;
        }
        self.cfg.is_input_provider()
            && (self.effective_plain_slide_peer_id() != self.applied_slide_peer_id
                || compute_deskflow_mouse_adjustment(&self.cfg, self.observed_monitor_input)
                    != self.applied_mouse_adjustment)
    }

    fn is_host_clamshell_closed(&self, host_id: &str) -> bool {
        if host_id == self.cfg.local_host_id {
            self.clamshell_closed
        } else {
            self.peer_clamshell_closed
                .get(host_id)
                .copied()
                .unwrap_or(false)
        }
    }

    /// When a laptop's lid is closed (`clamshell_closed == true`), it has no visible internal screen.
    /// If the 27" monitor is showing another online host (`h.monitor_input == inp`), return `(host_id, hotkey_slot)`.
    /// Note: any host actively displayed on the 27" monitor (`h.monitor_input == inp`) IS visible on that monitor.
    fn monitor_escape_target_for_closed_laptop(&self, closed_host_id: &str) -> Option<(String, u8)> {
        let inp = self.observed_monitor_input?;
        let mh = self
            .cfg
            .hosts
            .iter()
            .find(|h| h.enabled && h.monitor_input == inp && h.id != closed_host_id)?;
        let online = mh.id == self.cfg.local_host_id
            || self.peer_online.get(&mh.id).copied().unwrap_or(false);
        online.then(|| (mh.id.clone(), mh.hotkey_slot))
    }

    fn has_local_cursor_for_controls(&self) -> bool {
        if !self.cfg.deskflow_enabled || self.is_deskflow_session_locked() {
            return true;
        }
        let cursor_is_local = self
            .active_cursor_host
            .as_deref()
            .map(|id| id == self.cfg.local_host_id)
            .unwrap_or_else(|| self.cfg.is_input_provider());
        if self.deskflow_connected {
            return cursor_is_local;
        }
        if self.cfg.is_input_provider() {
            return true;
        }
        // Client during a brief Server restart should keep relying on active_cursor_host rather
        // than jumping to local=true mid-reload while the provider is still online.
        let provider_online = self
            .peer_online
            .get(&self.cfg.input_provider_host_id)
            .copied()
            .unwrap_or(false);
        if self.deskflow_running && provider_online {
            cursor_is_local
        } else {
            true
        }
    }

    fn apply_shared_reconcile(
        &mut self,
        incoming: &SharedClusterSettings,
        source_host_id: &str,
    ) -> (SharedSyncOutcome, bool) {
        let outcome = self.cfg.reconcile_shared_settings(incoming, source_host_id);
        let disabled_now = outcome.changed && !self.cfg.deskflow_enabled;
        if disabled_now {
            self.deskflow_running = false;
            self.deskflow_connected = false;
            self.active_cursor_host = Some(self.cfg.local_host_id.clone());
        }
        (outcome, disabled_now)
    }
}

pub fn compute_deskflow_mouse_adjustment(
    cfg: &AppConfig,
    observed_monitor_input: Option<u8>,
) -> Option<String> {
    if !cfg.is_input_provider() {
        return None;
    }
    let server_host = cfg.local_host().ok()?;
    let remote_hosts: Vec<&HostNode> = cfg
        .hosts
        .iter()
        .filter(|h| h.enabled && h.id != cfg.local_host_id)
        .collect();
    if remote_hosts.is_empty() {
        return None;
    }
    let client_host = if remote_hosts.len() == 1 {
        remote_hosts[0]
    } else {
        // When the 27" monitor is showing the local server host itself (or unknown),
        // prefer a same-platform peer first so Mac<->Mac stays 1:1 instead of falling back to Windows 2.00x.
        observed_monitor_input
            .and_then(|inp| remote_hosts.iter().copied().find(|h| h.monitor_input == inp))
            .or_else(|| {
                remote_hosts
                    .iter()
                    .copied()
                    .find(|h| h.platform == server_host.platform)
            })
            .unwrap_or(remote_hosts[0])
    };

    // Both macOS nodes report logical CoreGraphics HiDPI coordinates with identical native pointer
    // acceleration; never downscale Mac<->Mac deltas (e.g. 1920/2560 = 0.75) or restart Deskflow between Macs.
    if server_host.platform == HostPlatform::Macos && client_host.platform == HostPlatform::Macos {
        return None;
    }

    let sw = server_host.display_profile.preferred_width;
    let cw = client_host.display_profile.preferred_width;
    if sw > 0 && cw > 0 {
        let ratio = (cw as f64 / sw as f64).clamp(0.25, 4.0);
        if (ratio - 1.0).abs() < 0.05 {
            None
        } else {
            Some(format!("{ratio:.2}"))
        }
    } else {
        match (server_host.platform, client_host.platform) {
            (HostPlatform::Macos, HostPlatform::Windows) => Some("2.00".to_string()),
            (HostPlatform::Windows, HostPlatform::Macos) => Some("0.50".to_string()),
            _ => None,
        }
    }
}

pub struct AppRuntime {
    data_dir: PathBuf,
    shared_token: String,
    db: Arc<StateDb>,
    identity: TlsIdentity,
    client: LanControlClient,
    state: RwLock<LiveState>,
    ddc_notify: Notify,
    deskflow_reload_notify: Notify,
    standby_notify: Notify,
    picker_cb: RwLock<Option<EdgePickerCallback>>,
    blanker_cb: RwLock<Option<StandbyBlankerCallback>>,
    hud_cb: RwLock<Option<HudCallback>>,
}

pub fn default_data_dir() -> PathBuf {
    if let Ok(override_dir) = std::env::var("KVMDDC_DATA_DIR") {
        let trimmed = override_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    #[cfg(target_os = "windows")]
    if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local_app).join("Kvmddc");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".kvmddc");
    }
    std::env::temp_dir().join("kvmddc-data")
}

fn is_routable_pure_lan_ipv4(raw: &str) -> bool {
    let trimmed = raw.trim();
    let Ok(ip) = trimmed.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_link_local()
        && validate_pure_lan_ip(trimmed).is_ok()
}

fn select_preferred_local_pure_lan_ip(
    configured_ip: Option<&str>,
    active_ips: &[String],
    routed_probe_ip: Option<&str>,
    prefer_primary_nic: bool,
) -> Option<String> {
    let active_pure: Vec<String> = active_ips
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| is_routable_pure_lan_ipv4(s))
        .collect();

    if prefer_primary_nic {
        if let Some(primary) = active_pure.first() {
            return Some(primary.clone());
        }
    }

    if let Some(cfg_ip) = configured_ip.map(str::trim).filter(|s| !s.is_empty()) {
        if active_pure.iter().any(|ip| ip == cfg_ip) {
            return Some(cfg_ip.to_string());
        }
    }

    if let Some(probe) = routed_probe_ip.map(str::trim).filter(|s| !s.is_empty()) {
        if is_routable_pure_lan_ipv4(probe) {
            return Some(probe.to_string());
        }
    }

    active_pure.into_iter().next()
}

fn detect_local_pure_lan_ip(cfg: &AppConfig) -> Option<String> {
    let active_ips = kvmddc_platform::list_local_ipv4_addrs();
    let configured_ip = cfg.local_host().ok().map(|h| h.lan_ip.as_str());
    let mut routed_probe: Option<String> = None;

    let mut probe_targets: Vec<String> = cfg
        .hosts
        .iter()
        .filter(|h| h.id != cfg.local_host_id && !h.lan_ip.trim().is_empty())
        .map(|h| format!("{}:{}", h.lan_ip.trim(), cfg.control_port))
        .collect();
    probe_targets.push("192.168.2.1:80".to_string());
    probe_targets.push("192.168.1.1:80".to_string());

    for target in probe_targets {
        let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") else {
            continue;
        };
        if sock.connect(&target).is_ok() {
            if let Ok(addr) = sock.local_addr() {
                let ip_str = addr.ip().to_string();
                if is_routable_pure_lan_ipv4(&ip_str) {
                    routed_probe = Some(ip_str);
                    break;
                }
            }
        }
    }

    select_preferred_local_pure_lan_ip(
        configured_ip,
        &active_ips,
        routed_probe.as_deref(),
        cfg!(target_os = "macos"),
    )
}

fn filter_pure_lan_candidate_ips(
    addrs: impl IntoIterator<Item = std::net::SocketAddr>,
    exclude_ip: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    for addr in addrs {
        if let std::net::SocketAddr::V4(v4) = addr {
            let ip_str = v4.ip().to_string();
            if ip_str != exclude_ip
                && is_routable_pure_lan_ipv4(&ip_str)
                && !out.contains(&ip_str)
            {
                out.push(ip_str);
            }
        }
    }
    out
}

async fn resolve_peer_mdns_ips(mdns: Option<&str>, exclude_ip: &str, port: u16) -> Vec<String> {
    let Some(host) = mdns.map(str::trim).filter(|h| !h.is_empty()) else {
        return Vec::new();
    };
    let Ok(Ok(addrs)) = tokio::time::timeout(
        Duration::from_millis(800),
        tokio::net::lookup_host((host, port)),
    )
    .await
    else {
        return Vec::new();
    };
    filter_pure_lan_candidate_ips(addrs, exclude_ip)
}

fn sort_edge_candidates_for_picker(
    candidates: &mut [EdgeCandidateDto],
    observed_monitor_input: Option<u8>,
) {
    candidates.sort_by_key(|c| {
        let on_monitor_now = observed_monitor_input == Some(c.monitor_input);
        (!c.online, on_monitor_now, c.hotkey_slot)
    });
}

impl AppRuntime {
    pub fn initialize(data_dir: PathBuf) -> Result<Arc<Self>> {
        std::fs::create_dir_all(&data_dir)?;
        let mut cfg = AppConfig::load_or_init(&data_dir)?;
        let identity = load_or_generate_tls_identity(&data_dir, &cfg.local_host_id)?;

        // Ensure own TLS fingerprint and current DHCP LAN IP are persisted in config
        let mut dirty = false;
        let detected_lan_ip = detect_local_pure_lan_ip(&cfg);
        for h in &mut cfg.hosts {
            if h.id == cfg.local_host_id {
                if h.tls_fingerprint.as_deref() != Some(identity.v2_fingerprint.as_str()) {
                    h.tls_fingerprint = Some(identity.v2_fingerprint.clone());
                    dirty = true;
                }
                if let Some(ref fresh_ip) = detected_lan_ip {
                    if &h.lan_ip != fresh_ip {
                        h.lan_ip = fresh_ip.clone();
                        dirty = true;
                    }
                }
            }
        }
        let initial_probe = kvmddc_platform::probe_connected_monitor();
        let initial_modes = map_display_modes(kvmddc_platform::list_display_modes(true));
        let initial_ax = kvmddc_platform::is_accessibility_trusted();
        if let Some(det_in) = initial_probe.detected_input {
            if cfg.reconcile_detected_port(&cfg.local_host_id.clone(), det_in, false) {
                dirty = true;
            }
        }
        if dirty {
            cfg.save(&data_dir)?;
        }

        let db = Arc::new(StateDb::open(&data_dir.join("state.db"))?);
        let now = Utc::now();
        db.ensure_runtime(&cfg.local_host_id, cfg.preferred_brightness, now)?;
        let _ = db.recover_after_restart(&cfg.local_host_id, now)?;
        let _ = db.prune_older_than_days(30, now)?;
        let last_done_input = db.list_recent_requests(10).ok().and_then(|rows| {
            rows.into_iter().find_map(|r| {
                if matches!(r.status, kvmddc_core::db::SwitchStatus::Done) {
                    r.observed_input.or(Some(r.target_input))
                } else {
                    None
                }
            })
        });

        let mut peer_online = HashMap::new();
        peer_online.insert(cfg.local_host_id.clone(), true);

        let shared_token = cfg.shared_token.clone();
        let rt = Arc::new(Self {
            data_dir,
            shared_token,
            db,
            identity,
            client: LanControlClient::new()?,
            state: RwLock::new(LiveState {
                active_cursor_host: Some(cfg.input_provider_host_id.clone()),
                deskflow_screen_host: Some(cfg.local_host_id.clone()),
                cfg,
                deskflow_running: false,
                deskflow_connected: false,
                accessibility_trusted: initial_ax,
                display_sleeping: false,
                saved_ddc_brightness_before_sleep: None,
                external_display_online: initial_probe.connected
                    || kvmddc_platform::has_external_display(),
                observed_monitor_input: initial_probe.detected_input.or(last_done_input),
                local_probe: initial_probe,
                local_display_modes: initial_modes,
                peer_detected_inputs: HashMap::new(),
                peer_detected_ports: HashMap::new(),
                peer_detected_modes: HashMap::new(),
                peer_display_modes: HashMap::new(),
                peer_monitor_model: None,
                current_brightness: None,
                current_volume: None,
                current_color_preset: None,
                last_clipboard_png_sha: None,
                last_transfer_summary: None,
                edge_locked: kvmddc_platform::is_edge_locked(),
                peer_edge_locked: HashMap::new(),
                clamshell_closed: kvmddc_platform::is_clamshell_closed(),
                peer_clamshell_closed: HashMap::new(),
                applied_slide_peer_id: None,
                applied_mouse_adjustment: None,
                applied_confined_builtin: false,
                last_switch_activity_at: Instant::now() - Duration::from_secs(10),
                secure_input_active: false,
                last_error: None,
                peer_online,
                picker_visible: false,
                picker_bounds: None,
                edge_candidates: Vec::new(),
                hovered_candidate_id: None,
            }),
            ddc_notify: Notify::new(),
            deskflow_reload_notify: Notify::new(),
            standby_notify: Notify::new(),
            picker_cb: RwLock::new(None),
            blanker_cb: RwLock::new(None),
            hud_cb: RwLock::new(None),
        });
        Ok(rt)
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub async fn set_picker_callback(&self, cb: EdgePickerCallback) {
        *self.picker_cb.write().await = Some(cb);
    }

    pub async fn set_blanker_callback(&self, cb: StandbyBlankerCallback) {
        *self.blanker_cb.write().await = Some(cb);
    }

    pub async fn set_hud_callback(&self, cb: HudCallback) {
        *self.hud_cb.write().await = Some(cb);
    }

    async fn emit_hud(&self, kind: &str, label: impl Into<String>, value: u8, muted: bool) {
        if let Some(cb) = self.hud_cb.read().await.as_ref() {
            cb(HudSignal {
                kind: kind.to_string(),
                label: label.into(),
                value,
                muted,
            });
        }
    }

    pub fn spawn_background_loops(self: &Arc<Self>) {
        let s1 = Arc::clone(self);
        tokio::spawn(async move {
            s1.run_control_server_loop().await;
        });

        let s2 = Arc::clone(self);
        tokio::spawn(async move {
            s2.run_deskflow_supervisor_loop().await;
        });

        let s3 = Arc::clone(self);
        tokio::spawn(async move {
            s3.run_ddc_worker_loop().await;
        });

        let s4 = Arc::clone(self);
        tokio::spawn(async move {
            s4.run_peer_sync_loop().await;
        });

        let s5 = Arc::clone(self);
        tokio::spawn(async move {
            s5.run_display_guardian_loop().await;
        });

        let s6 = Arc::clone(self);
        tokio::spawn(async move {
            s6.run_edge_and_hotkey_loop().await;
        });

        let s7 = Arc::clone(self);
        tokio::spawn(async move {
            s7.run_standby_wake_loop().await;
        });

        let s8 = Arc::clone(self);
        tokio::spawn(async move {
            s8.run_clipboard_image_sync_loop().await;
        });

        let s9 = Arc::clone(self);
        tokio::spawn(async move {
            s9.run_hid_hotplug_loop().await;
        });
    }

    async fn refresh_local_lan_ip_if_needed(&self) -> bool {
        let fresh = {
            let st = self.state.read().await;
            detect_local_pure_lan_ip(&st.cfg)
        };
        let Some(new_ip) = fresh else {
            return false;
        };
        let changed = {
            let mut st = self.state.write().await;
            let my_id = st.cfg.local_host_id.clone();
            let mut updated = false;
            if let Some(h) = st.cfg.hosts.iter_mut().find(|h| h.id == my_id) {
                if h.lan_ip != new_ip {
                    h.lan_ip = new_ip;
                    updated = true;
                }
            }
            if updated {
                let _ = st.cfg.save(&self.data_dir);
            }
            updated
        };
        if changed {
            self.deskflow_reload_notify.notify_one();
        }
        changed
    }

    async fn run_control_server_loop(self: Arc<Self>) {
        loop {
            if self.refresh_local_lan_ip_if_needed().await {
                let sync_rt = Arc::clone(&self);
                tokio::spawn(async move {
                    sync_rt.sync_peer_settings().await;
                });
            }

            let (lan_ip, port) = {
                let st = self.state.read().await;
                let ip = st
                    .cfg
                    .local_host()
                    .map(|h| h.lan_ip.clone())
                    .unwrap_or_else(|_| "127.0.0.1".to_string());
                (ip, st.cfg.control_port)
            };

            match bind_pure_lan_listener(&lan_ip, port).await {
                Ok(listener) => {
                    let handler: Arc<dyn ControlPlaneHandler> =
                        Arc::clone(&self) as Arc<dyn ControlPlaneHandler>;
                    let router = build_control_router(handler);
                    let bound_ip = lan_ip.clone();
                    let watch_rt = Arc::clone(&self);
                    tokio::select! {
                        _ = axum::serve(listener, router) => {}
                        _ = async move {
                            loop {
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                let preferred = {
                                    let st = watch_rt.state.read().await;
                                    detect_local_pure_lan_ip(&st.cfg)
                                };
                                let active_addrs = kvmddc_platform::list_local_ipv4_addrs();
                                let bound_still_up =
                                    active_addrs.iter().any(|ip| ip.trim() == bound_ip);
                                let preferred_changed = preferred
                                    .as_deref()
                                    .map(|p| p != bound_ip)
                                    .unwrap_or(false);
                                if !bound_still_up || preferred_changed {
                                    break;
                                }
                            }
                        } => {}
                    }
                }
                Err(_) => {
                    if self.refresh_local_lan_ip_if_needed().await {
                        let sync_rt = Arc::clone(&self);
                        tokio::spawn(async move {
                            sync_rt.sync_peer_settings().await;
                        });
                        continue;
                    }
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            }
        }
    }

    async fn run_deskflow_supervisor_loop(self: Arc<Self>) {
        loop {
            let (cfg_snapshot, any_edge_locked, _ext_online, slide_peer, mouse_adj, confine_builtin, entered_confined) = {
                let mut st = self.state.write().await;
                let session_locked = st.is_deskflow_session_locked();
                let sp = st.effective_plain_slide_peer_id();
                let madj = compute_deskflow_mouse_adjustment(&st.cfg, st.observed_monitor_input);
                let prev_confined = st.applied_confined_builtin;
                let confine = st.should_confine_builtin_display();
                st.applied_slide_peer_id = sp.clone();
                st.applied_mouse_adjustment = madj.clone();
                st.applied_confined_builtin = confine;
                (
                    st.cfg.clone(),
                    session_locked,
                    st.external_display_online || st.observed_monitor_input.is_some(),
                    sp,
                    madj,
                    confine,
                    confine && !prev_confined,
                )
            };

            kvmddc_platform::set_builtin_display_confined(confine_builtin);
            sync_confine_builtin_flag(&self.data_dir, confine_builtin);
            if entered_confined {
                let _ = tokio::task::spawn_blocking(kvmddc_platform::clamp_cursor_to_builtin_if_needed).await;
            }

            if !cfg_snapshot.deskflow_enabled || any_edge_locked {
                let _ = tokio::task::spawn_blocking(|| {
                    kvmddc_platform::ensure_display_sleep_prevented(false);
                    kvmddc_platform::kill_orphan_deskflow_processes();
                })
                .await;
                {
                    let mut st = self.state.write().await;
                    st.deskflow_running = false;
                    st.deskflow_connected = false;
                    st.applied_slide_peer_id = None;
                    st.applied_mouse_adjustment = None;
                    st.applied_confined_builtin = false;
                    st.active_cursor_host = Some(st.cfg.local_host_id.clone());
                }
                self.deskflow_reload_notify.notified().await;
                continue;
            }

            let active_displays = tokio::task::spawn_blocking(|| {
                kvmddc_platform::ensure_display_sleep_prevented(true);
                kvmddc_platform::active_display_count()
            })
            .await
            .unwrap_or(1);
            if active_displays == 0 {
                // On headless / Studio nodes where macOS already timed out into Display Sleep, wait briefly
                // for WindowServer to wake `CGGetActiveDisplayList` before launching `deskflow-core`.
                for _ in 0..8 {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    let cnt = tokio::task::spawn_blocking(kvmddc_platform::active_display_count)
                        .await
                        .unwrap_or(0);
                    if cnt > 0 {
                        break;
                    }
                }
            }

            #[cfg(target_os = "macos")]
            {
                // Avoid a 5-second `deskflow-core` launch/crash loop when macOS Accessibility is
                // not yet trusted on this machine; keep the display sleep assertion held and wait
                // quietly until `run_display_guardian_loop` wakes us via `deskflow_reload_notify`.
                let ax_trusted = tokio::task::spawn_blocking(kvmddc_platform::is_accessibility_trusted)
                    .await
                    .unwrap_or(false);
                if !ax_trusted {
                    {
                        let mut st = self.state.write().await;
                        st.accessibility_trusted = false;
                        st.deskflow_running = false;
                        st.deskflow_connected = false;
                    }
                    tokio::select! {
                        _ = self.deskflow_reload_notify.notified() => {}
                        _ = tokio::time::sleep(Duration::from_secs(3)) => {}
                    }
                    continue;
                }
            }

            let bin_res = resolve_deskflow_binary(&cfg_snapshot);
            let conf_res = write_deskflow_runtime_files(
                &cfg_snapshot,
                &self.data_dir,
                &self.identity,
                slide_peer.as_deref(),
            );
            let tmp_res = create_isolated_short_tmpdir();

            let (Ok(bin_path), Ok((conf_path, is_server)), Ok(tmp_dir)) =
                (bin_res, conf_res, tmp_res)
            else {
                {
                    let mut st = self.state.write().await;
                    st.deskflow_running = false;
                    st.deskflow_connected = false;
                }
                tokio::select! {
                    _ = self.deskflow_reload_notify.notified() => {},
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {},
                }
                continue;
            };

            let _ = tokio::task::spawn_blocking(kvmddc_platform::kill_orphan_deskflow_processes)
                .await;

            let mode_arg = if is_server { "server" } else { "client" };
            let mut cmd = tokio::process::Command::new(&bin_path);
            cmd.args([
                mode_arg,
                "--new-instance",
                "-s",
                &conf_path.to_string_lossy(),
            ])
            .env("TMPDIR", &tmp_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

            if is_server {
                if let Some(ref adj) = mouse_adj {
                    cmd.env("DESKFLOW_MOUSE_ADJUSTMENT", adj);
                }
            }

            #[cfg(target_os = "macos")]
            {
                cmd.env(
                    "KVMDDC_CONFINED_BUILTIN_FLAG_FILE",
                    self.data_dir.join("deskflow/confine_builtin.flag"),
                );
                if confine_builtin {
                    cmd.env("KVMDDC_CONFINED_BUILTIN_ONLY", "1");
                }
                let shim = self.data_dir.join("bin/libtis_mainthread_shim.dylib");
                if shim.exists() {
                    cmd.env("DYLD_INSERT_LIBRARIES", shim);
                }
            }

            #[cfg(target_os = "windows")]
            {
                // CREATE_NO_WINDOW | ABOVE_NORMAL_PRIORITY_CLASS so background HID injection
                // never suffers desktop thread priority starvation.
                cmd.creation_flags(0x0800_0000 | 0x0000_8000);
            }

            let Ok(mut child) = cmd.spawn() else {
                let _ = std::fs::remove_dir_all(&tmp_dir);
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            };

            {
                let mut st = self.state.write().await;
                st.deskflow_running = true;
                st.deskflow_connected = false;
                st.deskflow_screen_host = Some(st.cfg.local_host_id.clone());
            }

            let missing_event_tap = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let missing_display_shape = Arc::new(std::sync::atomic::AtomicBool::new(false));
            if let Some(stdout) = child.stdout.take() {
                spawn_deskflow_output_reader(
                    stdout,
                    Arc::clone(&self),
                    Arc::clone(&missing_event_tap),
                    Arc::clone(&missing_display_shape),
                );
            }
            if let Some(stderr) = child.stderr.take() {
                spawn_deskflow_output_reader(
                    stderr,
                    Arc::clone(&self),
                    Arc::clone(&missing_event_tap),
                    Arc::clone(&missing_display_shape),
                );
            }

            let intentional_reload = tokio::select! {
                _ = self.deskflow_reload_notify.notified() => {
                    let _ = child.kill().await;
                    let _ = tokio::task::spawn_blocking(kvmddc_platform::kill_orphan_deskflow_processes).await;
                    true
                }
                _ = child.wait() => false,
            };

            {
                let mut st = self.state.write().await;
                st.deskflow_running = false;
                st.deskflow_connected = false;
            }
            let _ = std::fs::remove_dir_all(&tmp_dir);
            if missing_display_shape.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = tokio::task::spawn_blocking(kvmddc_platform::wake_idle_display_for_kvm)
                    .await;
                tokio::select! {
                    _ = self.deskflow_reload_notify.notified() => {},
                    _ = tokio::time::sleep(Duration::from_millis(1500)) => {},
                }
            } else if missing_event_tap.load(std::sync::atomic::Ordering::Relaxed) {
                tokio::select! {
                    _ = self.deskflow_reload_notify.notified() => {},
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {},
                }
            } else if intentional_reload {
                tokio::time::sleep(Duration::from_millis(80)).await;
            } else {
                tokio::time::sleep(Duration::from_millis(600)).await;
            }
        }
    }

    async fn ingest_deskflow_log(self: &Arc<Self>, line: &str) {
        let Some(ev) = parse_deskflow_log_line(line) else {
            return;
        };
        let refocus_target = {
            let mut st = self.state.write().await;
            st.apply_deskflow_log_event(ev)
        };

        if let Some((expected_id, slot)) = refocus_target {
            let seq = CLIENT_REFOCUS_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let rt_ref = Arc::clone(self);
            tokio::spawn(async move {
                for delay_ms in [120_u64, 420_u64, 900_u64] {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    if CLIENT_REFOCUS_SEQ.load(std::sync::atomic::Ordering::Relaxed) != seq {
                        break;
                    }
                    let still_wants = {
                        let st = rt_ref.state.read().await;
                        st.active_cursor_host.as_deref() == Some(expected_id.as_str())
                            && st.deskflow_screen_host.as_deref() != Some(expected_id.as_str())
                    };
                    if !still_wants {
                        break;
                    }
                    let _ = tokio::task::spawn_blocking(move || {
                        kvmddc_platform::trigger_internal_deskflow_slot(slot)
                    })
                    .await;
                }
            });
        }
    }

    async fn run_ddc_worker_loop(self: Arc<Self>) {
        let mut last_input_probe = Instant::now() - Duration::from_secs(5);
        let mut last_idle_probe = Instant::now() - Duration::from_secs(10);
        let mut idle_probe_ticks: u32 = 0;
        loop {
            let (is_ctrl, ctrl_id, settle_ms, pref_brightness, is_sleeping, need_extra_vcp, in_sw_grace) = {
                let st = self.state.read().await;
                (
                    st.cfg.is_ddc_controller(),
                    st.cfg.local_host_id.clone(),
                    st.cfg.settle_ms,
                    st.cfg.preferred_brightness,
                    st.display_sleeping,
                    st.current_volume.is_none() || st.current_color_preset.is_none(),
                    st.last_switch_activity_at.elapsed() < Duration::from_millis(1800),
                )
            };

            if !is_ctrl {
                tokio::select! {
                    _ = self.ddc_notify.notified() => {},
                    _ = tokio::time::sleep(Duration::from_secs(3)) => {},
                }
                continue;
            }

            let now = Utc::now();
            let _ = self.db.promote_pending_if_ready(&ctrl_id, settle_ms, now);

            if let Ok(Some(active)) = self.db.active_executing_request(&ctrl_id) {
                match active.status {
                    kvmddc_core::db::SwitchStatus::Writing => {
                        let target_in = active.target_input;
                        let mut write_res = tokio::task::spawn_blocking(move || {
                            if kvmddc_platform::read_input_source().ok() == Some(target_in) {
                                return Ok(());
                            }
                            kvmddc_platform::write_input_source(target_in)
                        })
                        .await
                        .unwrap_or_else(|e| Err(anyhow::anyhow!("线程异常: {e}")));

                        if write_res.is_err() {
                            let (peers, port, token, local_id) = {
                                let st = self.state.read().await;
                                let ps: Vec<HostNode> = st
                                    .cfg
                                    .hosts
                                    .iter()
                                    .filter(|h| {
                                        h.enabled
                                            && h.id != st.cfg.local_host_id
                                            && st.peer_online.get(&h.id).copied().unwrap_or(false)
                                    })
                                    .cloned()
                                    .collect();
                                (
                                    ps,
                                    st.cfg.control_port,
                                    st.cfg.shared_token.clone(),
                                    st.cfg.local_host_id.clone(),
                                )
                            };
                            let to_host = active
                                .to_host
                                .clone()
                                .unwrap_or_else(|| local_id.clone());
                            for peer in peers {
                                if self
                                    .client
                                    .request_switch(
                                        &peer.lan_ip,
                                        port,
                                        &token,
                                        &RemoteSwitchRequest {
                                            request_key: format!("{}-peer", active.request_key),
                                            source: active.source,
                                            from_host: active.from_host.clone(),
                                            to_host: to_host.clone(),
                                            target_input: target_in,
                                            sync_input_focus: false,
                                        },
                                    )
                                    .await
                                    .is_ok()
                                {
                                    write_res = Ok(());
                                    break;
                                }
                            }
                        }

                        let err_msg = write_res.err().map(|e| e.to_string());
                        if let Ok(updated) = self.db.record_ddc_write_attempt(
                            active.id,
                            &ctrl_id,
                            err_msg.as_deref(),
                            Utc::now(),
                        ) {
                            if updated.status == kvmddc_core::db::SwitchStatus::Failed {
                                let mut st = self.state.write().await;
                                st.last_error = updated.error;
                            }
                        }
                        continue;
                    }
                    kvmddc_core::db::SwitchStatus::Settling => {
                        if let Ok(Some(verifying_row)) =
                            self.db.begin_verifying_if_settled(&ctrl_id, settle_ms, Utc::now())
                        {
                            self.execute_ddc_verify(verifying_row, &ctrl_id).await;
                            continue;
                        }
                    }
                    kvmddc_core::db::SwitchStatus::Verifying => {
                        self.execute_ddc_verify(active, &ctrl_id).await;
                        continue;
                    }
                    _ => {}
                }

                tokio::select! {
                    _ = self.ddc_notify.notified() => {},
                    _ = tokio::time::sleep(Duration::from_millis(200)) => {},
                }
                continue;
            }

            // Fast 1.1s VCP 0x60 input source poll (detects physical monitor OSD switch & follows focus)
            // + full 25s idle brightness/volume/color-preset poll (skipped during standby or active switch).
            let has_pending = self.db.has_pending_request(&ctrl_id).unwrap_or(false);
            if !is_sleeping
                && !has_pending
                && !in_sw_grace
                && last_input_probe.elapsed() >= Duration::from_millis(1100)
            {
                last_input_probe = Instant::now();
                let do_full_probe = last_idle_probe.elapsed() >= Duration::from_secs(25);
                let poll_extra = if do_full_probe {
                    last_idle_probe = Instant::now();
                    let extra = need_extra_vcp || idle_probe_ticks.is_multiple_of(6);
                    idle_probe_ticks = idle_probe_ticks.wrapping_add(1);
                    extra
                } else {
                    false
                };
                let probe = tokio::task::spawn_blocking(move || {
                    let input = kvmddc_platform::read_input_source().ok();
                    if !do_full_probe {
                        return (input, None, None, None);
                    }
                    let mut bright = kvmddc_platform::read_brightness().ok();
                    // Auto-heal firmware brightness reset (e.g. Display P3 firmware reset to 100)
                    if let Some(cur_b) = bright {
                        if cur_b == 100 && pref_brightness < 100 {
                            if kvmddc_platform::write_brightness(pref_brightness).is_ok() {
                                bright = Some(pref_brightness);
                            }
                        }
                    }
                    let (vol, preset) = if poll_extra {
                        (
                            kvmddc_platform::read_volume().ok(),
                            kvmddc_platform::read_color_preset().ok(),
                        )
                    } else {
                        (None, None)
                    };
                    (input, bright, vol, preset)
                })
                .await
                .unwrap_or((None, None, None, None));

                if probe.0.is_some() || probe.1.is_some() || probe.2.is_some() || probe.3.is_some() {
                    let (reload_df, osd_focus, restore_presets) = {
                        let mut st = self.state.write().await;
                        let mut osd_focus_target: Option<(String, u8)> = None;
                        let mut presets_to_restore: Option<(Option<u8>, Option<u8>)> = None;
                        if !st.display_sleeping {
                            if let Some(inp) = probe.0 {
                                let prev_obs = st.observed_monitor_input;
                                if prev_obs != Some(inp) {
                                    st.observed_monitor_input = Some(inp);
                                    st.last_switch_activity_at = Instant::now();
                                    let confine = st.should_confine_builtin_display();
                                    st.applied_confined_builtin = confine;
                                    kvmddc_platform::set_builtin_display_confined(confine);
                                    sync_confine_builtin_flag(&self.data_dir, confine);

                                    if let Some(mh) = st
                                        .cfg
                                        .hosts
                                        .iter()
                                        .find(|h| h.enabled && h.monitor_input == inp)
                                        .cloned()
                                    {
                                        let mh_online = mh.id == st.cfg.local_host_id
                                            || st.peer_online.get(&mh.id).copied().unwrap_or(false);
                                        // Any host actively displayed on the 27" external monitor (`mh.monitor_input == inp`)
                                        // is visible there even when its built-in laptop lid is closed!
                                        if st.cfg.deskflow_enabled
                                            && mh_online
                                            && (prev_obs.is_some()
                                                || mh.id != st.cfg.local_host_id
                                                || st.active_cursor_host.as_deref()
                                                    != Some(mh.id.as_str()))
                                        {
                                            CLIENT_REFOCUS_SEQ
                                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                            st.active_cursor_host = Some(mh.id.clone());
                                            osd_focus_target = Some((mh.id.clone(), mh.hotkey_slot));
                                        }
                                        if prev_obs.is_some() {
                                            let (hb, hcp) =
                                                (mh.preferred_brightness, mh.preferred_color_preset);
                                            let mut dirty = false;
                                            if let Some(b) = hb {
                                                st.cfg.preferred_brightness = b;
                                                st.current_brightness = Some(b);
                                                dirty = true;
                                            }
                                            if let Some(cp) = hcp {
                                                st.cfg.preferred_color_preset = cp;
                                                st.current_color_preset = Some(cp);
                                                dirty = true;
                                            }
                                            if dirty {
                                                let _ = st.cfg.save(&self.data_dir);
                                            }
                                            if hb.is_some() || hcp.is_some() {
                                                presets_to_restore = Some((hb, hcp));
                                            }
                                        }
                                    }
                                }
                            }
                            if let Some(b) = probe.1 {
                                st.current_brightness = Some(b);
                            }
                            if let Some(v) = probe.2 {
                                st.current_volume = Some(v);
                            }
                            if let Some(cp) = probe.3 {
                                st.current_color_preset = Some(cp);
                            }
                        }
                        (
                            st.should_reload_deskflow_links(),
                            osd_focus_target,
                            presets_to_restore,
                        )
                    };
                    if reload_df {
                        self.deskflow_reload_notify.notify_one();
                    }
                    if let Some((fid, fslot)) = osd_focus {
                        self.focus_deskflow_on_host(&fid, fslot, &[350, 1100, 2200, 3400])
                            .await;
                    }
                    if let Some((hb, hcp)) = restore_presets {
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(220)).await;
                            let _ = tokio::task::spawn_blocking(move || {
                                if let Some(b) = hb {
                                    let _ = kvmddc_platform::write_brightness(b);
                                }
                                if let Some(cp) = hcp {
                                    let _ = kvmddc_platform::write_color_preset(cp);
                                }
                            })
                            .await;
                        });
                    }
                }
            }

            tokio::select! {
                _ = self.ddc_notify.notified() => {},
                _ = tokio::time::sleep(Duration::from_millis(950)) => {},
            }
        }
    }

    async fn execute_ddc_verify(&self, row: SwitchRequestRow, ctrl_id: &str) {
        let read_res = tokio::task::spawn_blocking(kvmddc_platform::read_input_source)
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("回读线程异常: {e}")));

        let (obs, err_str) = match read_res {
            Ok(v) => (Some(v), None),
            Err(e) => {
                let peer_confirmed = {
                    let st = self.state.read().await;
                    row.to_host.as_deref().is_some_and(|tid| {
                        tid != st.cfg.local_host_id
                            && st.peer_detected_inputs.get(tid).copied() == Some(row.target_input)
                    })
                };
                if peer_confirmed {
                    (Some(row.target_input), None)
                } else {
                    (None, Some(e.to_string()))
                }
            }
        };

        let mut restore_presets: Option<(Option<u8>, Option<u8>)> = None;
        if let Ok(outcome) = self.db.complete_verification(
            row.id,
            ctrl_id,
            obs,
            err_str.as_deref(),
            Utc::now(),
        ) {
            let mut st = self.state.write().await;
            match outcome {
                VerifyOutcome::Done(done_row) => {
                    st.observed_monitor_input = done_row.observed_input;
                    st.last_error = None;
                    let matched_presets = st
                        .cfg
                        .hosts
                        .iter()
                        .find(|h| {
                            done_row
                                .to_host
                                .as_deref()
                                .is_some_and(|tid| tid == h.id.as_str())
                                || done_row
                                    .observed_input
                                    .is_some_and(|inp| inp == h.monitor_input && h.enabled)
                        })
                        .map(|h| (h.preferred_brightness, h.preferred_color_preset));
                    if let Some((hb, hcp)) = matched_presets {
                        let mut dirty = false;
                        if let Some(b) = hb {
                            st.cfg.preferred_brightness = b;
                            st.current_brightness = Some(b);
                            dirty = true;
                        }
                        if let Some(cp) = hcp {
                            st.cfg.preferred_color_preset = cp;
                            st.current_color_preset = Some(cp);
                            dirty = true;
                        }
                        if dirty {
                            let _ = st.cfg.save(&self.data_dir);
                        }
                        if !st.display_sleeping && (hb.is_some() || hcp.is_some()) {
                            restore_presets = Some((hb, hcp));
                        }
                    }
                }
                VerifyOutcome::Failed(failed_row) => {
                    if let Some(v) = failed_row.observed_input {
                        st.observed_monitor_input = Some(v);
                    }
                    st.last_error = failed_row.error;
                }
                VerifyOutcome::RetryWrite(_) | VerifyOutcome::SupersededByPending(_) => {}
            }
            let confine = st.should_confine_builtin_display();
            st.applied_confined_builtin = confine;
            kvmddc_platform::set_builtin_display_confined(confine);
            sync_confine_builtin_flag(&self.data_dir, confine);
            let reload_df = st.should_reload_deskflow_links();
            drop(st);
            if reload_df {
                self.deskflow_reload_notify.notify_one();
            }
        }

        if let Some((hb, hcp)) = restore_presets {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(220)).await;
                let _ = tokio::task::spawn_blocking(move || {
                    if let Some(b) = hb {
                        let _ = kvmddc_platform::write_brightness(b);
                    }
                    if let Some(cp) = hcp {
                        let _ = kvmddc_platform::write_color_preset(cp);
                    }
                })
                .await;
            });
        }
    }

    async fn run_peer_sync_loop(self: Arc<Self>) {
        let mut peer_df_disconnect_streak: HashMap<String, u32> = HashMap::new();
        let mut peer_announced_ip: HashMap<String, String> = HashMap::new();
        loop {
            let (local_id, local_ip, port, token, peers, my_fp) = {
                let st = self.state.read().await;
                let local_ip = st
                    .cfg
                    .local_host()
                    .map(|h| h.lan_ip.clone())
                    .unwrap_or_default();
                let peers: Vec<HostNode> = st
                    .cfg
                    .hosts
                    .iter()
                    .filter(|h| h.id != st.cfg.local_host_id)
                    .cloned()
                    .collect();
                (
                    st.cfg.local_host_id.clone(),
                    local_ip,
                    st.cfg.control_port,
                    st.cfg.shared_token.clone(),
                    peers,
                    self.identity.v2_fingerprint.clone(),
                )
            };

            for peer in peers {
                let status_res = self.fetch_peer_status_with_mdns(&peer, port, &token).await;

                match status_res {
                    Ok(remote) => {
                        let mut need_pair = false;
                        let mut need_push_settings = false;
                        let mut hide_picker_now = false;
                        let mut cfg_dirty = false;
                        let mut peer_lock_changed = false;
                        let mut clamshell_rescue_slot: Option<u8> = None;
                        let mut push_cfg_snapshot: Option<AppConfig> = None;
                        let local_should_reconnect_df;
                        {
                            let mut st = self.state.write().await;
                            let incoming_shared = remote.to_shared_settings();
                            let (sync_out, disabled_now) =
                                st.apply_shared_reconcile(&incoming_shared, &remote.host_id);
                            if sync_out.changed {
                                cfg_dirty = true;
                            }
                            if sync_out.deskflow_reload_needed {
                                peer_lock_changed = true;
                            }
                            if disabled_now {
                                hide_picker_now = true;
                            }
                            if sync_out.push_back_to_source
                                || st.cfg.shared_settings_rev > remote.shared_settings_rev
                            {
                                need_push_settings = true;
                            }

                            if st.cfg.is_input_provider()
                                && st.deskflow_running
                                && !remote.is_input_provider
                                && remote.deskflow_connected
                            {
                                st.deskflow_connected = true;
                            }
                            local_should_reconnect_df = !st.cfg.is_input_provider()
                                && st.deskflow_running
                                && !st.deskflow_connected
                                && remote.host_id == st.cfg.input_provider_host_id
                                && remote.is_input_provider
                                && remote.deskflow_running;
                            st.peer_online.insert(peer.id.clone(), true);
                            if st
                                .peer_edge_locked
                                .insert(peer.id.clone(), remote.edge_locked)
                                .unwrap_or(false)
                                != remote.edge_locked
                            {
                                peer_lock_changed = true;
                            }
                            st.peer_clamshell_closed
                                .insert(peer.id.clone(), remote.clamshell_closed);
                            if remote.is_ddc_controller {
                                if remote.observed_monitor_input.is_some() {
                                    st.observed_monitor_input = remote.observed_monitor_input;
                                }
                                if remote.current_brightness.is_some() {
                                    st.current_brightness = remote.current_brightness;
                                }
                                if remote.current_volume.is_some() {
                                    st.current_volume = remote.current_volume;
                                }
                                if let Some(m) = remote.audio_muted {
                                    st.cfg.audio_muted = m;
                                }
                                if remote.current_color_preset.is_some() {
                                    st.current_color_preset = remote.current_color_preset;
                                }
                            } else if st.observed_monitor_input.is_none()
                                && remote.observed_monitor_input.is_some()
                            {
                                st.observed_monitor_input = remote.observed_monitor_input;
                            }
                            // Only accept cursor focus from the single synchronized input provider.
                            if !st.cfg.is_input_provider()
                                && remote.host_id == st.cfg.input_provider_host_id
                                && remote.is_input_provider
                                && remote.shared_settings_rev >= st.cfg.shared_settings_rev
                                && remote.active_cursor_host.is_some()
                            {
                                st.active_cursor_host = remote.active_cursor_host;
                            }
                            if let Some(m) = remote.detected_monitor_model.clone() {
                                if !m.is_empty() {
                                    st.peer_monitor_model = Some(m);
                                }
                            }
                            if let Some(pl) = remote.detected_port_label.clone() {
                                st.peer_detected_ports.insert(peer.id.clone(), pl);
                            } else {
                                st.peer_detected_ports.remove(&peer.id);
                            }
                            if let Some(ms) = remote.detected_mode_summary.clone() {
                                st.peer_detected_modes.insert(peer.id.clone(), ms);
                            } else {
                                st.peer_detected_modes.remove(&peer.id);
                            }
                            if !remote.display_modes.is_empty() {
                                if let Some(active_m) = remote.display_modes.iter().find(|m| {
                                    m.active && is_user_chosen_desktop_resolution(m.width, m.height)
                                }) {
                                    if let Some(h) =
                                        st.cfg.hosts.iter_mut().find(|h| h.id == peer.id)
                                    {
                                        if h.display_profile.preferred_width != active_m.width
                                            || h.display_profile.preferred_height != active_m.height
                                            || (active_m.hz >= 50
                                                && h.display_profile.preferred_hz != active_m.hz)
                                        {
                                            h.display_profile.preferred_width = active_m.width;
                                            h.display_profile.preferred_height = active_m.height;
                                            if active_m.hz >= 50 {
                                                h.display_profile.preferred_hz = active_m.hz;
                                            }
                                            cfg_dirty = true;
                                        }
                                    }
                                }
                                st.peer_display_modes
                                    .insert(peer.id.clone(), remote.display_modes.clone());
                            }
                            if let Some(det_code) = remote.detected_monitor_input {
                                st.peer_detected_inputs.insert(peer.id.clone(), det_code);
                                // NOTE: never copy remote.detected_monitor_input into observed_monitor_input;
                                // multi-host DP/HDMI cables always report connected ports even when inactive.
                                if st.cfg.reconcile_detected_port(&peer.id, det_code, true) {
                                    cfg_dirty = true;
                                    need_pair = true;
                                }
                            } else {
                                st.peer_detected_inputs.remove(&peer.id);
                                if let Some(h) = st.cfg.hosts.iter_mut().find(|h| h.id == peer.id) {
                                    if !h.enabled {
                                        h.enabled = true;
                                        cfg_dirty = true;
                                        need_pair = true;
                                    }
                                }
                            }
                            if let Some(h) = st.cfg.hosts.iter_mut().find(|h| h.id == peer.id) {
                                if !remote.tls_fingerprint.is_empty()
                                    && h.tls_fingerprint.as_deref()
                                        != Some(remote.tls_fingerprint.as_str())
                                {
                                    h.tls_fingerprint = Some(remote.tls_fingerprint.clone());
                                    cfg_dirty = true;
                                    need_pair = true;
                                }
                            }
                            if st.cfg.is_input_provider()
                                && st.cfg.deskflow_enabled
                                && st.deskflow_running
                                && st.last_switch_activity_at.elapsed()
                                    >= Duration::from_millis(5000)
                            {
                                if let Some(cur_id) = st.active_cursor_host.clone() {
                                    let cur_is_closed_laptop = st
                                        .cfg
                                        .hosts
                                        .iter()
                                        .any(|h| h.id == cur_id && h.display_profile.has_internal_screen)
                                        && st.is_host_clamshell_closed(&cur_id);
                                    if cur_is_closed_laptop {
                                        if let Some((esc_id, esc_slot)) =
                                            st.monitor_escape_target_for_closed_laptop(&cur_id)
                                        {
                                            st.active_cursor_host = Some(esc_id);
                                            st.last_switch_activity_at = Instant::now();
                                            clamshell_rescue_slot = Some(esc_slot);
                                        }
                                    }
                                }
                            }
                            if cfg_dirty {
                                let _ = st.cfg.save(&self.data_dir);
                            }
                            if need_push_settings {
                                push_cfg_snapshot = Some(st.cfg.clone());
                            }
                            let confine = st.should_confine_builtin_display();
                            st.applied_confined_builtin = confine;
                            kvmddc_platform::set_builtin_display_confined(confine);
                            sync_confine_builtin_flag(&self.data_dir, confine);
                            if st.should_reload_deskflow_links() {
                                peer_lock_changed = true;
                            }
                        }
                        if hide_picker_now {
                            self.hide_edge_picker().await;
                        }
                        if let Some(slot) = clamshell_rescue_slot {
                            let _ = tokio::task::spawn_blocking(move || {
                                kvmddc_platform::trigger_internal_deskflow_slot(slot)
                            })
                            .await;
                        }
                        if peer_lock_changed {
                            self.deskflow_reload_notify.notify_one();
                        }
                        if local_should_reconnect_df {
                            let c = peer_df_disconnect_streak.entry(peer.id.clone()).or_insert(0);
                            *c += 1;
                            if *c >= 3 {
                                *c = 0;
                                self.deskflow_reload_notify.notify_one();
                            }
                        } else {
                            peer_df_disconnect_streak.insert(peer.id.clone(), 0);
                        }
                        let need_ip_announce =
                            peer_announced_ip.get(&peer.id) != Some(&local_ip);
                        if need_push_settings || need_pair || peer.tls_fingerprint.is_none() || need_ip_announce {
                            let pair_req = if let Some(ref cfg_snap) = push_cfg_snapshot {
                                PairPeerRequest::from_config(cfg_snap, local_ip.clone(), my_fp.clone())
                            } else {
                                PairPeerRequest::announce_only(local_id.clone(), local_ip.clone(), my_fp.clone())
                            };
                            if self
                                .client
                                .pair(&peer.lan_ip, port, &token, &pair_req)
                                .await
                                .is_ok()
                            {
                                peer_announced_ip.insert(peer.id.clone(), local_ip.clone());
                            }
                            if need_pair || peer.tls_fingerprint.is_none() {
                                self.deskflow_reload_notify.notify_one();
                            }
                        }
                    }
                    Err(_) => {
                        let reload_df = {
                            let mut st = self.state.write().await;
                            st.peer_online.insert(peer.id.clone(), false);
                            let had_lock = st.peer_edge_locked.remove(&peer.id).unwrap_or(false);
                            st.peer_clamshell_closed.remove(&peer.id);
                            had_lock || st.should_reload_deskflow_links()
                        };
                        if reload_df {
                            self.deskflow_reload_notify.notify_one();
                        }
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(1200)).await;
        }
    }

    async fn run_display_guardian_loop(self: Arc<Self>) {
        let mut last_mode_sig: Option<(bool, Option<u32>, Option<u32>, Option<u32>)> = None;
        loop {
            let (pref_w, pref_h, pref_hz, modes_cached) = {
                let st = self.state.read().await;
                let modes_cached = !st.local_display_modes.is_empty();
                match st.cfg.local_host() {
                    Ok(h) => (
                        h.display_profile.preferred_width,
                        h.display_profile.preferred_height,
                        h.display_profile.preferred_hz,
                        modes_cached,
                    ),
                    Err(_) => (0, 0, 0, modes_cached),
                }
            };

            let prev_sig = last_mode_sig;
            let (locked, clamshell, sec_in, probe, modes, ax_ok, next_sig) =
                tokio::task::spawn_blocking(move || {
                    let mut p = kvmddc_platform::probe_connected_monitor();
                    // Only heal broken VGA/EDID fallbacks (< 1280×720). Never overwrite a valid
                    // desktop resolution that the user chose in macOS / Windows Display Settings!
                    let broken_fallback = p
                        .active_width
                        .zip(p.active_height)
                        .is_some_and(|(w, h)| !is_user_chosen_desktop_resolution(w, h));
                    if is_user_chosen_desktop_resolution(pref_w, pref_h)
                        && p.connected
                        && broken_fallback
                    {
                        if let Ok(Some(_)) =
                            kvmddc_platform::heal_display_profile(pref_w, pref_h, pref_hz)
                        {
                            p = kvmddc_platform::probe_connected_monitor();
                        }
                    }
                    let cur_sig = (p.connected, p.active_width, p.active_height, p.active_hz);
                    let m = if p.connected && (!modes_cached || prev_sig != Some(cur_sig)) {
                        map_display_modes(kvmddc_platform::list_display_modes(true))
                    } else {
                        Vec::new()
                    };
                    (
                        kvmddc_platform::is_edge_locked(),
                        kvmddc_platform::is_clamshell_closed(),
                        kvmddc_platform::is_secure_input_enabled(),
                        p,
                        m,
                        kvmddc_platform::is_accessibility_trusted(),
                        cur_sig,
                    )
                })
                .await
                .unwrap_or((
                    false,
                    false,
                    false,
                    ConnectedMonitorProbe::default(),
                    Vec::new(),
                    true,
                    (false, None, None, None),
                ));
            last_mode_sig = Some(next_sig);

            let ext_online = probe.connected;
            let mut reload_df = false;
            let mut lid_escape: Option<(String, u8, Option<(String, u16, String)>)> = None;
            {
                let mut st = self.state.write().await;
                if !st.accessibility_trusted && ax_ok {
                    reload_df = true;
                }
                st.accessibility_trusted = ax_ok;
                let was_clamshell = st.clamshell_closed;
                st.clamshell_closed = clamshell;
                if st.external_display_online != ext_online {
                    st.external_display_online = ext_online;
                }
                let my_id = st.cfg.local_host_id.clone();
                let mut cfg_dirty = false;
                if let Some(det_in) = probe.detected_input {
                    if st.observed_monitor_input.is_none() {
                        st.observed_monitor_input = Some(det_in);
                    }
                    if st.cfg.reconcile_detected_port(&my_id, det_in, false) {
                        cfg_dirty = true;
                    }
                }
                if probe.connected {
                    if let (Some(w), Some(h)) = (probe.active_width, probe.active_height) {
                        if is_user_chosen_desktop_resolution(w, h) {
                            let hz = probe.active_hz.unwrap_or(0);
                            if let Some(lh) = st.cfg.hosts.iter_mut().find(|n| n.id == my_id) {
                                if lh.display_profile.preferred_width != w
                                    || lh.display_profile.preferred_height != h
                                    || (hz >= 50 && lh.display_profile.preferred_hz != hz)
                                {
                                    lh.display_profile.preferred_width = w;
                                    lh.display_profile.preferred_height = h;
                                    if hz >= 50 {
                                        lh.display_profile.preferred_hz = hz;
                                    }
                                    cfg_dirty = true;
                                }
                            }
                        }
                    }
                }
                if cfg_dirty {
                    let _ = st.cfg.save(&self.data_dir);
                }
                st.local_probe = probe;
                if !modes.is_empty() {
                    st.local_display_modes = modes;
                }
                if ext_online
                    && st
                        .last_error
                        .as_deref()
                        .is_some_and(|e| e.contains("未检测到"))
                {
                    st.last_error = None;
                }
                // If lid just closed while 27" monitor is showing another online host (even if
                // USB-C keeps ext_online=true without sleep), immediately hand focus to that host.
                if !was_clamshell && clamshell && st.cfg.deskflow_enabled {
                    let local_id = st.cfg.local_host_id.clone();
                    let cur_on_laptop = st
                        .active_cursor_host
                        .as_deref()
                        .map(|id| id == local_id)
                        .unwrap_or(true);
                    if cur_on_laptop {
                        if let Some((esc_id, esc_slot)) =
                            st.monitor_escape_target_for_closed_laptop(&local_id)
                        {
                            st.active_cursor_host = Some(esc_id.clone());
                            st.last_switch_activity_at = Instant::now();
                            let remote_provider = if st.cfg.is_input_provider() {
                                None
                            } else {
                                st.cfg
                                    .hosts
                                    .iter()
                                    .find(|h| h.id == st.cfg.input_provider_host_id && h.enabled)
                                    .map(|h| {
                                        (
                                            h.lan_ip.clone(),
                                            st.cfg.control_port,
                                            st.cfg.shared_token.clone(),
                                        )
                                    })
                            };
                            lid_escape = Some((esc_id, esc_slot, remote_provider));
                        }
                    }
                }
                if st.edge_locked != locked {
                    st.edge_locked = locked;
                    reload_df = true;
                }
                let confine = st.should_confine_builtin_display();
                st.applied_confined_builtin = confine;
                kvmddc_platform::set_builtin_display_confined(confine);
                sync_confine_builtin_flag(&self.data_dir, confine);
                if st.should_reload_deskflow_links() {
                    reload_df = true;
                }
                st.secure_input_active = sec_in;
            }

            if let Some((esc_id, esc_slot, remote_provider)) = lid_escape {
                if let Some((prov_ip, port, token)) = remote_provider {
                    let _ = self
                        .client
                        .focus_input(
                            &prov_ip,
                            port,
                            &token,
                            &FocusInputRequest {
                                target_host_id: esc_id,
                                hotkey_slot: esc_slot,
                            },
                        )
                        .await;
                } else {
                    let _ = tokio::task::spawn_blocking(move || {
                        kvmddc_platform::trigger_internal_deskflow_slot(esc_slot)
                    })
                    .await;
                }
            }

            if reload_df {
                self.deskflow_reload_notify.notify_one();
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    async fn run_edge_and_hotkey_loop(self: Arc<Self>) {
        let mut edge_since: Option<(EdgeSide, Instant)> = None;
        let mut last_triggered = Instant::now() - Duration::from_secs(5);
        let mut require_rearm = false;
        let mut require_ctrl_release = false;
        let mut prev_local_cursor = true;
        let mut prev_mouse_down = false;
        let mut seen_dismissed_seq =
            PICKER_DISMISSED_SEQ.load(std::sync::atomic::Ordering::Relaxed);

        loop {
            if let Some(slot) = kvmddc_platform::poll_user_hotkey_slot() {
                let target_info = {
                    let st = self.state.read().await;
                    let bri = st.current_brightness.unwrap_or(st.cfg.preferred_brightness);
                    st.cfg
                        .hosts
                        .iter()
                        .find(|h| h.enabled && h.hotkey_slot == slot)
                        .map(|h| {
                            (
                                h.id.clone(),
                                h.display_name.clone(),
                                h.preferred_brightness.unwrap_or(bri),
                            )
                        })
                };
                if let Some((tid, tname, tbri)) = target_info {
                    let _ = self.orchestrate_switch(&tid, SwitchSource::Hotkey).await;
                    self.emit_hud("host", format!("切至 {tname}"), tbri, false)
                        .await;
                    last_triggered = Instant::now();
                }
            }

            if let Some(media_hk) = kvmddc_platform::poll_media_adjust_hotkey() {
                let (act_here, cur_b, cur_v, cur_m) = {
                    let st = self.state.read().await;
                    (
                        st.has_local_cursor_for_controls(),
                        st.current_brightness.unwrap_or(st.cfg.preferred_brightness),
                        st.current_volume.unwrap_or(st.cfg.preferred_volume),
                        st.cfg.audio_muted,
                    )
                };
                if act_here {
                    match media_hk {
                        MediaAdjustHotkey::BrightnessUp => {
                            let next = cur_b.saturating_add(6).min(100);
                            if self.set_brightness_global(next).await.is_ok() {
                                self.emit_hud("brightness", "显示器亮度", next, false).await;
                            }
                        }
                        MediaAdjustHotkey::BrightnessDown => {
                            let next = cur_b.saturating_sub(6);
                            if self.set_brightness_global(next).await.is_ok() {
                                self.emit_hud("brightness", "显示器亮度", next, false).await;
                            }
                        }
                        MediaAdjustHotkey::VolumeUp => {
                            let next = cur_v.saturating_add(6).min(100);
                            if self.set_volume_global(Some(next), Some(false)).await.is_ok() {
                                self.emit_hud("volume", "显示器音量", next, false).await;
                            }
                        }
                        MediaAdjustHotkey::VolumeDown => {
                            let next = cur_v.saturating_sub(6);
                            if self.set_volume_global(Some(next), Some(false)).await.is_ok() {
                                self.emit_hud("volume", "显示器音量", next, false).await;
                            }
                        }
                        MediaAdjustHotkey::ToggleMute => {
                            let next_m = !cur_m;
                            if self.set_volume_global(None, Some(next_m)).await.is_ok() {
                                let label = if next_m { "显示器静音" } else { "显示器音量" };
                                self.emit_hud("volume", label, cur_v, next_m).await;
                            }
                        }
                    }
                }
            }

            let (
                dwell_ms,
                switch_delay_ms,
                ctrl_edge_active,
                picker_open,
                picker_bounds,
                has_local_cursor,
                in_switch_cooldown,
                confine_builtin,
                plain_slide_active,
            ) = {
                let st = self.state.read().await;
                let any_locked = st.is_deskflow_session_locked();
                let mon_ready =
                    st.external_display_online || st.observed_monitor_input.is_some();
                let local_cursor = st.has_local_cursor_for_controls();
                (
                    st.cfg.ctrl_edge_dwell_ms,
                    st.cfg.deskflow_switch_delay_ms,
                    st.cfg.deskflow_enabled
                        && st.cfg.ctrl_edge_switch_enabled
                        && !any_locked
                        && !st.display_sleeping
                        && mon_ready,
                    st.picker_visible,
                    st.picker_bounds,
                    local_cursor,
                    st.last_switch_activity_at.elapsed() < Duration::from_millis(4500),
                    st.should_confine_builtin_display(),
                    st.effective_plain_slide_peer_id().is_some(),
                )
            };

            let cur_dismissed_seq =
                PICKER_DISMISSED_SEQ.load(std::sync::atomic::Ordering::Relaxed);
            if cur_dismissed_seq != seen_dismissed_seq {
                seen_dismissed_seq = cur_dismissed_seq;
                require_rearm = true;
                require_ctrl_release = true;
                edge_since = None;
            }

            kvmddc_platform::set_builtin_display_confined(confine_builtin);

            if !prev_local_cursor && has_local_cursor {
                // Cursor just landed on this host (often parked at x=0/right edge); require clean Ctrl/edge release before arming
                require_rearm = true;
                edge_since = None;
            }
            prev_local_cursor = has_local_cursor;
            if in_switch_cooldown {
                require_rearm = true;
                edge_since = None;
            }

            if !ctrl_edge_active {
                if picker_open {
                    self.hide_edge_picker().await;
                }
                edge_since = None;
                prev_mouse_down = false;
            } else if let Some(mut sample) = kvmddc_platform::sample_pointer_and_ctrl() {
                if confine_builtin
                    && has_local_cursor
                    && !in_switch_cooldown
                    && (!plain_slide_active || sample.ctrl_down)
                    && (sample.x < sample.screen_left
                        || sample.x >= sample.screen_right
                        || sample.y < sample.screen_top
                        || sample.y >= sample.screen_bottom)
                {
                    if kvmddc_platform::clamp_cursor_to_builtin_if_needed() {
                        if let Some(updated) = kvmddc_platform::sample_pointer_and_ctrl() {
                            sample = updated;
                        }
                    }
                }
                if !sample.ctrl_down {
                    require_ctrl_release = false;
                }
                if require_rearm
                    && !require_ctrl_release
                    && (!sample.ctrl_down || sample.near_edge(16).is_none())
                {
                    require_rearm = false;
                }

                let mouse_just_pressed = sample.mouse_down && !prev_mouse_down;
                prev_mouse_down = sample.mouse_down;

                let clicked_outside_picker = picker_open
                    && mouse_just_pressed
                    && picker_bounds.is_some_and(|(l, t, r, b)| {
                        sample.x < l || sample.x >= r || sample.y < t || sample.y >= b
                    });

                if clicked_outside_picker {
                    self.cancel_edge_picker().await;
                    seen_dismissed_seq =
                        PICKER_DISMISSED_SEQ.load(std::sync::atomic::Ordering::Relaxed);
                    require_rearm = true;
                    require_ctrl_release = true;
                    edge_since = None;
                } else if picker_open && !sample.ctrl_down {
                    if let Some(tid) = self.release_confirm_edge_picker().await {
                        let _ = self.orchestrate_switch(&tid, SwitchSource::EdgePicker).await;
                        last_triggered = Instant::now();
                        require_rearm = true;
                    }
                    seen_dismissed_seq =
                        PICKER_DISMISSED_SEQ.load(std::sync::atomic::Ordering::Relaxed);
                    edge_since = None;
                } else if sample.ctrl_down
                    && has_local_cursor
                    && !in_switch_cooldown
                    && !require_rearm
                    && !require_ctrl_release
                {
                    let edge_margin = if sample.screen_right - sample.screen_left >= 3840 {
                        8
                    } else {
                        4
                    };
                    if let Some(side) = sample
                        .near_edge(edge_margin)
                        .filter(|&s| kvmddc_platform::is_outer_desktop_edge(sample.x, sample.y, s))
                    {
                        let start = match edge_since {
                            Some((prev_side, ts)) if prev_side == side => ts,
                            _ => {
                                let now_i = Instant::now();
                                edge_since = Some((side, now_i));
                                now_i
                            }
                        };

                        // Trigger Ctrl edge slightly faster than Deskflow's switchDelay so
                        // Ctrl+push opens Edge Picker instead of plain-sliding across the seam.
                        let effective_dwell_ms = if plain_slide_active {
                            dwell_ms
                                .min((switch_delay_ms as u64).saturating_sub(30))
                                .clamp(70, 220)
                        } else {
                            dwell_ms.clamp(140, 220)
                        };
                        if !picker_open
                            && start.elapsed() >= Duration::from_millis(effective_dwell_ms)
                            && last_triggered.elapsed() >= Duration::from_millis(1800)
                        {
                            let candidates = self.collect_other_enabled_candidates().await;
                            if candidates.len() == 1 {
                                last_triggered = Instant::now();
                                require_rearm = true;
                                require_ctrl_release = true;
                                edge_since = None;
                                let _ = self
                                    .orchestrate_switch(&candidates[0].id, SwitchSource::CtrlEdge)
                                    .await;
                            } else if candidates.len() >= 2 {
                                // Align flush to active screen edge so the cursor at x=0 / right-1
                                // immediately sits inside the picker window for instant wheel scroll.
                                let popup_x = match side {
                                    EdgeSide::Left => sample.screen_left,
                                    EdgeSide::Right => sample.screen_right - 1,
                                };
                                let popup_y = (sample.y - 58).clamp(
                                    sample.screen_top + 12,
                                    (sample.screen_bottom - 180).max(sample.screen_top + 12),
                                );
                                self.show_edge_picker(candidates, popup_x, popup_y).await;
                            }
                        }
                    } else {
                        edge_since = None;
                    }
                } else {
                    edge_since = None;
                }
            }

            tokio::time::sleep(Duration::from_millis(35)).await;
        }
    }

    async fn collect_other_enabled_candidates(&self) -> Vec<EdgeCandidateDto> {
        let st = self.state.read().await;
        st.cfg
            .hosts
            .iter()
            .filter(|h| h.enabled && h.id != st.cfg.local_host_id)
            .map(|h| EdgeCandidateDto {
                id: h.id.clone(),
                display_name: h.display_name.clone(),
                monitor_input: h.monitor_input,
                hotkey_slot: h.hotkey_slot,
                online: st.peer_online.get(&h.id).copied().unwrap_or(false),
            })
            .collect()
    }

    async fn show_edge_picker(&self, mut candidates: Vec<EdgeCandidateDto>, x: i32, y: i32) {
        {
            let mut st = self.state.write().await;
            sort_edge_candidates_for_picker(&mut candidates, st.observed_monitor_input);
            st.hovered_candidate_id = candidates.first().map(|c| c.id.clone());
            st.edge_candidates = candidates;
            st.picker_bounds = Some((x, y, x + 256, y + 172));
            st.picker_visible = true;
        }
        if let Some(cb) = self.picker_cb.read().await.as_ref() {
            cb(EdgePickerSignal::Show { x, y });
        }
    }

    pub async fn set_picker_bounds(&self, bounds: (i32, i32, i32, i32)) {
        let mut st = self.state.write().await;
        if st.picker_visible {
            st.picker_bounds = Some(bounds);
        }
    }

    pub async fn hide_edge_picker(&self) {
        {
            let mut st = self.state.write().await;
            st.picker_visible = false;
            st.picker_bounds = None;
        }
        if let Some(cb) = self.picker_cb.read().await.as_ref() {
            cb(EdgePickerSignal::Hide);
        }
    }

    pub async fn cancel_edge_picker(&self) {
        let was_visible = {
            let mut st = self.state.write().await;
            let vis = st.picker_visible;
            st.picker_visible = false;
            st.picker_bounds = None;
            st.hovered_candidate_id = None;
            vis
        };
        PICKER_DISMISSED_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if was_visible {
            if let Some(cb) = self.picker_cb.read().await.as_ref() {
                cb(EdgePickerSignal::Hide);
            }
        }
    }

    async fn release_confirm_edge_picker(&self) -> Option<String> {
        let chosen = {
            let mut st = self.state.write().await;
            if !st.picker_visible {
                return None;
            }
            st.picker_visible = false;
            st.picker_bounds = None;
            st.hovered_candidate_id.take()
        };
        PICKER_DISMISSED_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Some(cb) = self.picker_cb.read().await.as_ref() {
            cb(EdgePickerSignal::Hide);
        }
        chosen
    }

    pub async fn get_edge_candidates(&self) -> Vec<EdgeCandidateDto> {
        let st = self.state.read().await;
        if st.edge_candidates.is_empty() {
            drop(st);
            return self.collect_other_enabled_candidates().await;
        }
        st.edge_candidates.clone()
    }

    pub async fn hover_edge_candidate(&self, host_id: String) {
        let mut st = self.state.write().await;
        if st.picker_visible {
            st.hovered_candidate_id = Some(host_id);
        }
    }

    pub async fn confirm_edge_pick(&self, host_id: String) -> Result<DashboardStateDto> {
        let should_switch = {
            let mut st = self.state.write().await;
            let vis = st.picker_visible;
            st.picker_visible = false;
            st.picker_bounds = None;
            st.hovered_candidate_id = None;
            vis
        };
        PICKER_DISMISSED_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Some(cb) = self.picker_cb.read().await.as_ref() {
            cb(EdgePickerSignal::Hide);
        }
        if should_switch {
            self.orchestrate_switch(&host_id, SwitchSource::EdgePicker)
                .await?;
        }
        self.dashboard_snapshot().await
    }

    async fn focus_deskflow_on_host(
        &self,
        target_host_id: &str,
        slot: u8,
        followups_ms: &'static [u64],
    ) {
        let (df_on, is_provider, provider_opt, port, token) = {
            let st = self.state.read().await;
            let prov = st
                .cfg
                .hosts
                .iter()
                .find(|h| h.id == st.cfg.input_provider_host_id && h.enabled)
                .map(|h| h.lan_ip.clone());
            (
                st.cfg.deskflow_enabled,
                st.cfg.is_input_provider(),
                prov,
                st.cfg.control_port,
                st.cfg.shared_token.clone(),
            )
        };
        if !df_on {
            return;
        }
        CLIENT_REFOCUS_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut guard) = FOCUS_FOLLOWUP_TARGET.lock() {
            *guard = Some(target_host_id.to_string());
        }
        let focus_seq = FOCUS_FOLLOWUP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if is_provider {
            let _ = tokio::task::spawn_blocking(move || {
                kvmddc_platform::trigger_internal_deskflow_slot(slot)
            })
            .await;
            if !followups_ms.is_empty() {
                tokio::spawn(async move {
                    let mut prev_ms = 0_u64;
                    for &at_ms in followups_ms {
                        let wait_ms = at_ms.saturating_sub(prev_ms);
                        prev_ms = at_ms;
                        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
                        if FOCUS_FOLLOWUP_SEQ.load(std::sync::atomic::Ordering::Relaxed)
                            != focus_seq
                        {
                            break;
                        }
                        let _ = tokio::task::spawn_blocking(move || {
                            kvmddc_platform::trigger_internal_deskflow_slot(slot)
                        })
                        .await;
                    }
                });
            }
        } else if let Some(prov_ip) = provider_opt {
            let _ = self
                .client
                .focus_input(
                    &prov_ip,
                    port,
                    &token,
                    &FocusInputRequest {
                        target_host_id: target_host_id.to_string(),
                        hotkey_slot: slot,
                    },
                )
                .await;
        }
    }

    pub async fn orchestrate_switch(
        &self,
        to_host_id: &str,
        source: SwitchSource,
    ) -> Result<ActionAck> {
        let (
            local_id,
            local_ip,
            target_host,
            provider_host,
            controller_host,
            port,
            token,
            settle_ms,
            target_online,
        ) = {
            let st = self.state.read().await;
            let local_h = st.cfg.local_host()?.clone();
            let target = st
                .cfg
                .hosts
                .iter()
                .find(|h| h.id == to_host_id && h.enabled)
                .cloned()
                .with_context(|| format!("目标主机未启用或不存在: {to_host_id}"))?;
            let provider = st
                .cfg
                .hosts
                .iter()
                .find(|h| h.id == st.cfg.input_provider_host_id && h.enabled)
                .cloned()
                .context("找不到键鼠提供端配置")?;
            let controller = st
                .cfg
                .hosts
                .iter()
                .find(|h| h.id == st.cfg.primary_ddc_controller_id && h.enabled)
                .cloned()
                .context("找不到 DDC 控制端配置")?;
            let online = st.peer_online.get(&target.id).copied().unwrap_or(target.id == local_h.id);
            (
                local_h.id,
                local_h.lan_ip,
                target,
                provider,
                controller,
                st.cfg.control_port,
                st.cfg.shared_token.clone(),
                st.cfg.settle_ms,
                online,
            )
        };

        // 1. If target is offline and has WoL MAC configured, send magic packet first
        if !target_online && target_host.id != local_id {
            if let Some(mac) = &target_host.mac_address {
                if let Ok(lip) = validate_pure_lan_ip(&local_ip) {
                    let oct = lip.octets();
                    let bcast = Ipv4Addr::new(oct[0], oct[1], oct[2], 255);
                    let _ = send_wol_broadcast(lip, bcast, mac);
                }
            }
        }

        let (was_sleeping, prev_observed_input, local_ext_online) = {
            let st = self.state.read().await;
            (
                st.display_sleeping,
                st.observed_monitor_input,
                st.external_display_online,
            )
        };

        // 2. Wake displays before switching input source so monitor sees active link signal
        let _ = self.exit_local_standby().await;
        if target_host.id != local_id {
            let _ = self.client.wake(&target_host.lan_ip, port, &token).await;
        }

        // 3. Switch Deskflow keyboard/mouse focus to target slot when Deskflow guardian is enabled.
        // When `controller_host != local_id` AND `provider_host == controller_host`, bundle focus
        // atomically inside `POST /v1/switch (sync_input_focus: true)` so the provider updates
        // `observed_monitor_input` and `active_cursor_host` under one lock without a torn window.
        let delegate_focus_to_controller =
            controller_host.id != local_id && provider_host.id == controller_host.id;
        CLIENT_REFOCUS_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let reload_df = {
            let mut st = self.state.write().await;
            st.last_switch_activity_at = Instant::now();
            st.observed_monitor_input = Some(target_host.monitor_input);
            st.active_cursor_host = Some(target_host.id.clone());
            let confine = st.should_confine_builtin_display();
            st.applied_confined_builtin = confine;
            kvmddc_platform::set_builtin_display_confined(confine);
            sync_confine_builtin_flag(&self.data_dir, confine);
            st.should_reload_deskflow_links()
        };
        if reload_df {
            self.deskflow_reload_notify.notify_one();
        }
        if !delegate_focus_to_controller {
            self.focus_deskflow_on_host(
                &target_host.id,
                target_host.hotkey_slot,
                &[250, 800, 1800, 3000],
            )
            .await;
        }

        // 4. Enqueue DDC 0x60 switch on primary DDC controller (with local fallback)
        if controller_host.id == local_id {
            if !was_sleeping {
                let live_in = tokio::task::spawn_blocking(kvmddc_platform::read_input_source)
                    .await
                    .ok()
                    .and_then(|r| r.ok());
                let already_active = match live_in {
                    Some(cur) => cur == target_host.monitor_input,
                    None => {
                        if target_host.id == local_id {
                            local_ext_online && prev_observed_input == Some(target_host.monitor_input)
                        } else {
                            !local_ext_online
                                && prev_observed_input == Some(target_host.monitor_input)
                        }
                    }
                };
                if already_active {
                    return Ok(ActionAck {
                        ok: true,
                        status: "already_active".to_string(),
                        message: "当前已是该设备画面，无需重复切屏".to_string(),
                    });
                }
            }
            let req_key = uuid::Uuid::new_v4().to_string();
            let out = self.db.enqueue_switch(
                NewSwitchRequest {
                    request_key: req_key,
                    source,
                    controller: local_id.clone(),
                    from_host: Some(local_id),
                    to_host: Some(target_host.id),
                    target_input: target_host.monitor_input,
                },
                settle_ms,
                Utc::now(),
            )?;
            self.ddc_notify.notify_one();
            let status_name = match out {
                kvmddc_core::db::EnqueueOutcome::DuplicateIgnored(r) => r.status.as_str().to_string(),
                kvmddc_core::db::EnqueueOutcome::ReadyToWrite(r) => r.status.as_str().to_string(),
                kvmddc_core::db::EnqueueOutcome::QueuedPending { row, .. } => {
                    row.status.as_str().to_string()
                }
            };
            return Ok(ActionAck {
                ok: true,
                status: status_name,
                message: "已进入本机 DDC 切屏队列".to_string(),
            });
        }

        let req_key = uuid::Uuid::new_v4().to_string();
        let remote_res = self
            .client
            .request_switch(
                &controller_host.lan_ip,
                port,
                &token,
                &RemoteSwitchRequest {
                    request_key: req_key,
                    source,
                    from_host: Some(local_id),
                    to_host: target_host.id.clone(),
                    target_input: target_host.monitor_input,
                    sync_input_focus: delegate_focus_to_controller,
                },
            )
            .await;

        match remote_res {
            Ok(ack) => Ok(ack),
            Err(remote_err) => {
                // Fallback to local DDC (e.g. Windows dxva2 when MacBook controller is away)
                let target_in = target_host.monitor_input;
                let local_ddc = tokio::task::spawn_blocking(move || {
                    kvmddc_platform::write_input_source(target_in)
                })
                .await
                .unwrap_or_else(|e| Err(anyhow::anyhow!("{e}")));

                match local_ddc {
                    Ok(()) => Ok(ActionAck {
                        ok: true,
                        status: "fallback_written".to_string(),
                        message: "主控离线，已通过本机兜底写入输入源".to_string(),
                    }),
                    Err(_) => Err(remote_err),
                }
            }
        }
    }

    pub async fn set_brightness_global(&self, brightness: u8) -> Result<()> {
        if brightness > 100 {
            bail!("亮度需在 0..=100 之间");
        }
        if brightness > 0 && self.state.read().await.display_sleeping {
            let _ = self.wake_displays_all().await;
        }
        let (is_ctrl, local_id, ctrl_ip, port, token, cur_preset, ext_online) = {
            let mut st = self.state.write().await;
            st.cfg.preferred_brightness = brightness;
            let obs_in = st.observed_monitor_input;
            let cur_host = st.active_cursor_host.clone();
            if let Some(h) =
                active_display_host_mut(&mut st.cfg, obs_in, cur_host.as_deref())
            {
                h.preferred_brightness = Some(brightness);
            }
            st.cfg.save(&self.data_dir)?;
            let ctrl_ip = st
                .cfg
                .hosts
                .iter()
                .find(|h| h.id == st.cfg.primary_ddc_controller_id)
                .map(|h| h.lan_ip.clone())
                .unwrap_or_default();
            let cp = st
                .current_color_preset
                .unwrap_or(st.cfg.preferred_color_preset);
            (
                st.cfg.is_ddc_controller(),
                st.cfg.local_host_id.clone(),
                ctrl_ip,
                st.cfg.control_port,
                st.cfg.shared_token.clone(),
                cp,
                st.external_display_online,
            )
        };

        self.db
            .set_preferred_brightness(&local_id, brightness, Utc::now())?;

        if is_ctrl {
            let _ = tokio::task::spawn_blocking(move || {
                let ddc_ok = ext_online && kvmddc_platform::write_brightness(brightness).is_ok();
                if !ddc_ok || brightness < 15 {
                    let _ = kvmddc_platform::apply_software_gamma(
                        brightness,
                        cur_preset,
                        ext_online,
                    );
                } else {
                    let _ = kvmddc_platform::apply_software_gamma(100, 1, ext_online);
                }
            })
            .await;
            self.state.write().await.current_brightness = Some(brightness);
        } else if !ctrl_ip.is_empty() {
            if self
                .client
                .set_brightness(&ctrl_ip, port, &token, brightness)
                .await
                .is_err()
            {
                let _ = tokio::task::spawn_blocking(move || {
                    if kvmddc_platform::write_brightness(brightness).is_err() {
                        let _ = kvmddc_platform::apply_software_gamma(
                            brightness,
                            cur_preset,
                            ext_online,
                        );
                    }
                })
                .await;
            }
            self.state.write().await.current_brightness = Some(brightness);
        }
        Ok(())
    }

    pub async fn set_volume_global(
        &self,
        volume: Option<u8>,
        muted: Option<bool>,
    ) -> Result<DashboardStateDto> {
        if let Some(v) = volume {
            ensure!(v <= 100, "音量需在 0..=100 之间");
        }
        let (is_ctrl, ctrl_ip, port, token, target_vol, target_muted, apply_mute) = {
            let mut st = self.state.write().await;
            let mut apply_m = muted;
            if let Some(v) = volume {
                st.cfg.preferred_volume = v;
                if v > 0 && muted.is_none() && st.cfg.audio_muted {
                    st.cfg.audio_muted = false;
                    apply_m = Some(false);
                }
            }
            if let Some(m) = apply_m {
                st.cfg.audio_muted = m;
            }
            st.cfg.save(&self.data_dir)?;
            let ctrl_ip = st
                .cfg
                .hosts
                .iter()
                .find(|h| h.id == st.cfg.primary_ddc_controller_id)
                .map(|h| h.lan_ip.clone())
                .unwrap_or_default();
            (
                st.cfg.is_ddc_controller(),
                ctrl_ip,
                st.cfg.control_port,
                st.cfg.shared_token.clone(),
                st.cfg.preferred_volume,
                st.cfg.audio_muted,
                apply_m,
            )
        };

        if is_ctrl {
            let v_opt = volume;
            let m_opt = apply_mute;
            let ok = tokio::task::spawn_blocking(move || {
                let mut success = true;
                if let Some(v) = v_opt {
                    if kvmddc_platform::write_volume(v).is_err() {
                        success = false;
                    }
                }
                if let Some(m) = m_opt {
                    if kvmddc_platform::write_audio_mute(m).is_err() {
                        success = false;
                    }
                }
                success
            })
            .await
            .unwrap_or(false);
            if ok || volume.is_some() {
                let mut st = self.state.write().await;
                st.current_volume = Some(target_vol);
                st.cfg.audio_muted = target_muted;
            }
        } else if !ctrl_ip.is_empty() {
            let req = VolumeRequest {
                volume,
                muted: apply_mute,
            };
            if self
                .client
                .set_volume(&ctrl_ip, port, &token, &req)
                .await
                .is_err()
            {
                let v_opt = volume;
                let m_opt = apply_mute;
                let _ = tokio::task::spawn_blocking(move || {
                    if let Some(v) = v_opt {
                        let _ = kvmddc_platform::write_volume(v);
                    }
                    if let Some(m) = m_opt {
                        let _ = kvmddc_platform::write_audio_mute(m);
                    }
                })
                .await;
            }
            let mut st = self.state.write().await;
            st.current_volume = Some(target_vol);
            st.cfg.audio_muted = target_muted;
        }

        self.dashboard_snapshot().await
    }

    pub async fn set_color_preset_global(&self, preset: u8) -> Result<()> {
        ensure!(preset >= 1, "色温预设编码非法");
        let (is_ctrl, ctrl_ip, port, token, cur_bri, ext_online) = {
            let mut st = self.state.write().await;
            st.cfg.preferred_color_preset = preset;
            let obs_in = st.observed_monitor_input;
            let cur_host = st.active_cursor_host.clone();
            if let Some(h) =
                active_display_host_mut(&mut st.cfg, obs_in, cur_host.as_deref())
            {
                h.preferred_color_preset = Some(preset);
            }
            st.cfg.save(&self.data_dir)?;
            let ctrl_ip = st
                .cfg
                .hosts
                .iter()
                .find(|h| h.id == st.cfg.primary_ddc_controller_id)
                .map(|h| h.lan_ip.clone())
                .unwrap_or_default();
            let cb = st.current_brightness.unwrap_or(st.cfg.preferred_brightness);
            (
                st.cfg.is_ddc_controller(),
                ctrl_ip,
                st.cfg.control_port,
                st.cfg.shared_token.clone(),
                cb,
                st.external_display_online,
            )
        };

        if is_ctrl {
            let _ = tokio::task::spawn_blocking(move || {
                let ddc_ok = ext_online && kvmddc_platform::write_color_preset(preset).is_ok();
                if !ddc_ok || cur_bri < 15 {
                    let _ = kvmddc_platform::apply_software_gamma(
                        cur_bri,
                        preset,
                        ext_online,
                    );
                } else {
                    let _ = kvmddc_platform::apply_software_gamma(100, 1, ext_online);
                }
            })
            .await;
            self.state.write().await.current_color_preset = Some(preset);
        } else if !ctrl_ip.is_empty() {
            if self
                .client
                .set_color_preset(&ctrl_ip, port, &token, preset)
                .await
                .is_err()
            {
                let _ = tokio::task::spawn_blocking(move || {
                    if kvmddc_platform::write_color_preset(preset).is_err() {
                        let _ = kvmddc_platform::apply_software_gamma(
                            cur_bri,
                            preset,
                            ext_online,
                        );
                    }
                })
                .await;
            }
            self.state.write().await.current_color_preset = Some(preset);
        }
        Ok(())
    }

    pub async fn select_color_preset(&self, preset: u8) -> Result<DashboardStateDto> {
        self.set_color_preset_global(preset).await?;
        self.emit_hud("color", color_preset_label(preset), preset, false)
            .await;
        self.dashboard_snapshot().await
    }

    pub async fn select_display_mode(
        &self,
        host_id: Option<String>,
        mode_id: i32,
        width: u32,
        height: u32,
        hz: u32,
        hidpi: bool,
    ) -> Result<DashboardStateDto> {
        ensure!(width >= 640 && height >= 480, "无效的显示分辨率");
        let (target_id, target_ip, is_local, port, token) = {
            let mut st = self.state.write().await;
            let resolved_id = if let Some(hid) = host_id.filter(|s| !s.trim().is_empty()) {
                hid
            } else {
                let obs_in = st.observed_monitor_input;
                let cur_host = st.active_cursor_host.clone();
                active_display_host_mut(&mut st.cfg, obs_in, cur_host.as_deref())
                    .map(|h| h.id.clone())
                    .unwrap_or_else(|| st.cfg.local_host_id.clone())
            };
            let target_h = st
                .cfg
                .hosts
                .iter_mut()
                .find(|h| h.id == resolved_id)
                .with_context(|| format!("未知目标主机: {resolved_id}"))?;
            target_h.display_profile.preferred_width = width;
            target_h.display_profile.preferred_height = height;
            if hz > 0 {
                target_h.display_profile.preferred_hz = hz;
            }
            let tip = target_h.lan_ip.clone();
            st.cfg.save(&self.data_dir)?;
            (
                resolved_id.clone(),
                tip,
                resolved_id == st.cfg.local_host_id,
                st.cfg.control_port,
                st.cfg.shared_token.clone(),
            )
        };

        let hud_label = if is_local {
            let (applied, probe, modes) = tokio::task::spawn_blocking(move || -> Result<_> {
                let app =
                    kvmddc_platform::apply_display_mode(mode_id, width, height, hz, hidpi, true)?;
                let p = kvmddc_platform::probe_connected_monitor();
                let m = map_display_modes(kvmddc_platform::list_display_modes(true));
                Ok((app, p, m))
            })
            .await
            .context("本地显示模式切换线程异常")??;

            {
                let mut st = self.state.write().await;
                st.local_probe = probe;
                if !modes.is_empty() {
                    st.local_display_modes = modes;
                }
                if let Some(h) = st.cfg.hosts.iter_mut().find(|h| h.id == target_id) {
                    h.display_profile.preferred_width = applied.width;
                    h.display_profile.preferred_height = applied.height;
                    if applied.hz > 0 {
                        h.display_profile.preferred_hz = applied.hz;
                    }
                    let _ = st.cfg.save(&self.data_dir);
                }
            }
            applied.label
        } else {
            let ack = self
                .client
                .set_display_mode(
                    &target_ip,
                    port,
                    &token,
                    &DisplayModeRequest {
                        mode_id,
                        width,
                        height,
                        hz,
                        hidpi,
                    },
                )
                .await?;
            let summary = format!("{}×{} · {}Hz", width, height, hz.max(60));
            {
                let mut st = self.state.write().await;
                st.peer_detected_modes
                    .insert(target_id.clone(), summary.clone());
                if let Some(list) = st.peer_display_modes.get_mut(&target_id) {
                    for item in list.iter_mut() {
                        item.active = item.width == width
                            && item.height == height
                            && (hz == 0 || item.hz == hz)
                            && item.hidpi == hidpi;
                    }
                }
            }
            if !ack.message.is_empty() {
                ack.message
            } else {
                summary
            }
        };

        let (reload_df, refocus_after_mode) = {
            let mut st = self.state.write().await;
            let mut refocus = None;
            if let Some(th) = st
                .cfg
                .hosts
                .iter()
                .find(|h| h.enabled && h.id == target_id)
                .cloned()
            {
                let on_shared_monitor = st.observed_monitor_input == Some(th.monitor_input);
                let had_focus = st.active_cursor_host.as_deref() == Some(target_id.as_str());
                if st.cfg.deskflow_enabled && (on_shared_monitor || had_focus) {
                    st.active_cursor_host = Some(target_id.clone());
                    st.last_switch_activity_at = Instant::now();
                    refocus = Some((target_id.clone(), th.hotkey_slot));
                }
            }
            (st.should_reload_deskflow_links(), refocus)
        };
        if reload_df {
            self.deskflow_reload_notify.notify_one();
        }
        if let Some((fid, fslot)) = refocus_after_mode {
            self.focus_deskflow_on_host(&fid, fslot, &[350, 1200, 2200])
                .await;
        }

        self.emit_hud("mode", hud_label, 100, false).await;
        self.dashboard_snapshot().await
    }

    pub async fn cycle_color_preset(&self) -> Result<DashboardStateDto> {
        let cur = {
            let st = self.state.read().await;
            st.current_color_preset.unwrap_or(st.cfg.preferred_color_preset)
        };
        let next = next_color_preset(cur);
        self.select_color_preset(next).await
    }

    pub async fn toggle_cross_os_modifiers(&self) -> Result<DashboardStateDto> {
        {
            let mut st = self.state.write().await;
            if !st.cfg.deskflow_enabled {
                drop(st);
                return self.dashboard_snapshot().await;
            }
            st.cfg.unify_cross_os_modifiers = !st.cfg.unify_cross_os_modifiers;
            st.cfg.bump_shared_settings_rev();
            st.cfg.save(&self.data_dir)?;
        }
        self.deskflow_reload_notify.notify_one();
        self.sync_peer_settings().await;
        self.dashboard_snapshot().await
    }

    pub async fn set_deskflow_enabled(&self, enabled: bool) -> Result<DashboardStateDto> {
        {
            let mut st = self.state.write().await;
            st.cfg.deskflow_enabled = enabled;
            st.cfg.bump_shared_settings_rev();
            if !enabled {
                st.deskflow_running = false;
                st.deskflow_connected = false;
                st.active_cursor_host = Some(st.cfg.local_host_id.clone());
            }
            st.cfg.save(&self.data_dir)?;
        }
        if !enabled {
            self.hide_edge_picker().await;
            let _ = tokio::task::spawn_blocking(kvmddc_platform::kill_orphan_deskflow_processes)
                .await;
        }
        self.deskflow_reload_notify.notify_one();
        self.sync_peer_settings().await;
        self.dashboard_snapshot().await
    }

    pub async fn toggle_ctrl_edge_switch(&self) -> Result<DashboardStateDto> {
        {
            let mut st = self.state.write().await;
            if !st.cfg.deskflow_enabled {
                drop(st);
                return self.dashboard_snapshot().await;
            }
            st.cfg.ctrl_edge_switch_enabled = !st.cfg.ctrl_edge_switch_enabled;
            st.cfg.bump_shared_settings_rev();
            st.cfg.save(&self.data_dir)?;
        }
        self.sync_peer_settings().await;
        self.dashboard_snapshot().await
    }

    pub async fn send_paths_to_host(
        &self,
        target_host_id: String,
        raw_paths: Vec<String>,
    ) -> Result<DashboardStateDto> {
        ensure!(!raw_paths.is_empty(), "未选择待传输文件");
        let (target_host, port, token) = {
            let st = self.state.read().await;
            ensure!(
                target_host_id != st.cfg.local_host_id,
                "文件已在本机，无需跨机传输"
            );
            let target = st
                .cfg
                .hosts
                .iter()
                .find(|h| h.id == target_host_id && h.enabled)
                .cloned()
                .with_context(|| format!("目标主机未启用或不存在: {target_host_id}"))?;
            (target, st.cfg.control_port, st.cfg.shared_token.clone())
        };

        let entries = tokio::task::spawn_blocking(move || collect_transfer_entries(&raw_paths))
            .await
            .context("扫描待传输文件线程异常")??;
        ensure!(!entries.is_empty(), "未找到可传输的常规文件");

        let total_files = entries.len();
        let first_name = entries[0].display_leaf.clone();
        for entry in entries {
            let src_path = entry.abs_path;
            let rel_path = entry.rel_path;
            let bytes = tokio::task::spawn_blocking(move || std::fs::read(&src_path))
                .await
                .context("读取本地文件线程异常")?
                .with_context(|| format!("读取文件失败: {rel_path}"))?;
            self.client
                .transfer_file(&target_host.lan_ip, port, &token, &rel_path, bytes)
                .await?;
        }

        let summary = if total_files == 1 {
            format!("已传至 {} · {first_name}", target_host.display_name)
        } else {
            format!(
                "已传至 {} · {total_files} 个文件",
                target_host.display_name
            )
        };
        self.state.write().await.last_transfer_summary = Some(summary.clone());
        self.emit_hud("transfer", &summary, 100, false).await;
        self.dashboard_snapshot().await
    }

    async fn run_clipboard_image_sync_loop(self: Arc<Self>) {
        if let Ok(Some(initial_png)) =
            tokio::task::spawn_blocking(kvmddc_platform::read_clipboard_png).await
        {
            let sha = sha256_hex_bytes(&initial_png);
            self.state.write().await.last_clipboard_png_sha = Some(sha);
        }

        loop {
            tokio::time::sleep(Duration::from_millis(1800)).await;

            let (online_peer_ips, port, token) = {
                let st = self.state.read().await;
                let ips: Vec<String> = st
                    .cfg
                    .hosts
                    .iter()
                    .filter(|h| {
                        h.enabled
                            && h.id != st.cfg.local_host_id
                            && st.peer_online.get(&h.id).copied().unwrap_or(false)
                    })
                    .map(|h| h.lan_ip.clone())
                    .collect();
                (ips, st.cfg.control_port, st.cfg.shared_token.clone())
            };

            if online_peer_ips.is_empty() {
                continue;
            }

            let Ok(Some(png_bytes)) =
                tokio::task::spawn_blocking(kvmddc_platform::read_clipboard_png).await
            else {
                continue;
            };
            if png_bytes.is_empty() || png_bytes.len() > 32 * 1024 * 1024 {
                continue;
            }

            let sha = sha256_hex_bytes(&png_bytes);
            let is_new = {
                let mut st = self.state.write().await;
                if st.last_clipboard_png_sha.as_deref() == Some(sha.as_str()) {
                    false
                } else {
                    st.last_clipboard_png_sha = Some(sha);
                    true
                }
            };
            if !is_new {
                continue;
            }

            for ip in online_peer_ips {
                let client = self.client.clone();
                let tok = token.clone();
                let payload = png_bytes.clone();
                tokio::spawn(async move {
                    let _ = client.send_clipboard_png(&ip, port, &tok, payload).await;
                });
            }
        }
    }

    async fn run_hid_hotplug_loop(self: Arc<Self>) {
        let mut tracker = InputHotplugTracker::new();
        loop {
            let (auto_switch, deskflow_enabled, local_id, current_provider) = {
                let st = self.state.read().await;
                (
                    st.cfg.auto_switch_input_provider_on_hotplug,
                    st.cfg.deskflow_enabled,
                    st.cfg.local_host_id.clone(),
                    st.cfg.input_provider_host_id.clone(),
                )
            };
            let eligible = auto_switch && deskflow_enabled && current_provider != local_id;

            let probe = tokio::task::spawn_blocking(|| {
                let devices = list_external_input_devices();
                let ax_trusted = kvmddc_platform::is_accessibility_trusted();
                (devices, ax_trusted)
            })
            .await;

            if let Ok((devices, ax_trusted)) = probe {
                if let Some(claimed) =
                    tracker.poll(Instant::now(), &devices, eligible, ax_trusted)
                {
                    let summary: Vec<String> = claimed
                        .iter()
                        .map(|d| format!("{} ({})", d.product, d.transport))
                        .collect();
                    eprintln!(
                        "[kvmddc] hid hotplug claimed provider={local_id}: {}",
                        summary.join(", ")
                    );
                    if let Err(err) = self.set_input_provider(local_id.clone()).await {
                        eprintln!("[kvmddc] hid hotplug failed to set provider={local_id}: {err}");
                    }
                }
            }

            tokio::time::sleep(HID_HOTPLUG_POLL_INTERVAL).await;
        }
    }

    pub async fn enter_local_standby(&self) {
        let should_dim_ddc = {
            let mut st = self.state.write().await;
            let cur_b = st
                .current_brightness
                .unwrap_or(st.cfg.preferred_brightness)
                .max(10);
            if !st.display_sleeping {
                st.saved_ddc_brightness_before_sleep = Some(cur_b);
            }
            st.display_sleeping = true;
            st.cfg.is_ddc_controller() || st.local_probe.connected
        };

        if let Some(cb) = self.blanker_cb.read().await.as_ref() {
            cb(true);
        }
        self.standby_notify.notify_one();

        let _ = tokio::task::spawn_blocking(move || {
            let _ = kvmddc_platform::sleep_displays_keep_awake();
            if should_dim_ddc {
                let _ = kvmddc_platform::write_brightness(0);
            }
        })
        .await;
    }

    pub async fn exit_local_standby(&self) -> bool {
        let (was_sleeping, restore_b, should_restore_ddc) = {
            let mut st = self.state.write().await;
            let was = st.display_sleeping;
            st.display_sleeping = false;
            let rb = st
                .saved_ddc_brightness_before_sleep
                .take()
                .unwrap_or(st.cfg.preferred_brightness)
                .max(10);
            if was {
                st.current_brightness = Some(rb);
            }
            (was, rb, st.cfg.is_ddc_controller() || st.local_probe.connected)
        };

        if let Some(cb) = self.blanker_cb.read().await.as_ref() {
            cb(false);
        }

        let awake = tokio::task::spawn_blocking(kvmddc_platform::wake_displays_and_wait)
            .await
            .unwrap_or(Ok(true))
            .unwrap_or(true);

        if was_sleeping && should_restore_ddc {
            tokio::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    let _ = kvmddc_platform::write_brightness(restore_b);
                })
                .await;
                tokio::time::sleep(Duration::from_millis(450)).await;
                let _ = tokio::task::spawn_blocking(move || {
                    let _ = kvmddc_platform::write_brightness(restore_b);
                })
                .await;
            });
        }

        awake
    }

    pub async fn sleep_displays_all(&self) -> Result<DashboardStateDto> {
        self.enter_local_standby().await;

        let (peers, port, token) = {
            let st = self.state.read().await;
            let peers: Vec<String> = st
                .cfg
                .hosts
                .iter()
                .filter(|h| h.enabled && h.id != st.cfg.local_host_id)
                .map(|h| h.lan_ip.clone())
                .collect();
            (peers, st.cfg.control_port, st.cfg.shared_token.clone())
        };

        for ip in peers {
            let client = self.client.clone();
            let tok = token.clone();
            tokio::spawn(async move {
                let _ = client.display_sleep(&ip, port, &tok).await;
            });
        }
        self.dashboard_snapshot().await
    }

    pub async fn wake_displays_all(&self) -> Result<DashboardStateDto> {
        let _ = self.exit_local_standby().await;

        let (peers, port, token) = {
            let st = self.state.read().await;
            let peers: Vec<String> = st
                .cfg
                .hosts
                .iter()
                .filter(|h| h.enabled && h.id != st.cfg.local_host_id)
                .map(|h| h.lan_ip.clone())
                .collect();
            (peers, st.cfg.control_port, st.cfg.shared_token.clone())
        };

        for ip in peers {
            let client = self.client.clone();
            let tok = token.clone();
            tokio::spawn(async move {
                let _ = client.wake(&ip, port, &tok).await;
            });
        }
        self.dashboard_snapshot().await
    }

    async fn run_standby_wake_loop(self: Arc<Self>) {
        loop {
            if !self.state.read().await.display_sleeping {
                self.standby_notify.notified().await;
            }
            if !self.state.read().await.display_sleeping {
                continue;
            }

            let mut detector = StandbyWakeDetector::new();
            while self.state.read().await.display_sleeping {
                if detector.poll_triggered() {
                    let _ = self.wake_displays_all().await;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
        }
    }

    pub async fn refresh_accessibility_trust(&self) -> bool {
        let ax_ok = kvmddc_platform::is_accessibility_trusted();
        let mut reload_df = false;
        {
            let mut st = self.state.write().await;
            if !st.accessibility_trusted && ax_ok {
                reload_df = true;
            }
            st.accessibility_trusted = ax_ok;
        }
        if reload_df {
            self.deskflow_reload_notify.notify_one();
        }
        ax_ok
    }

    pub async fn set_input_provider(&self, host_id: String) -> Result<DashboardStateDto> {
        {
            let mut st = self.state.write().await;
            let exists = st.cfg.hosts.iter().any(|h| h.id == host_id);
            ensure!(exists, "未知键鼠直连主机: {host_id}");
            if let Some(h) = st.cfg.hosts.iter_mut().find(|h| h.id == host_id) {
                h.enabled = true;
            }
            st.cfg.input_provider_host_id = host_id;
            st.cfg.bump_shared_settings_rev();
            if let Some(inp) = st.observed_monitor_input {
                if let Some(mh) = st
                    .cfg
                    .hosts
                    .iter()
                    .find(|h| h.enabled && h.monitor_input == inp)
                    .map(|h| h.id.clone())
                {
                    let online = mh == st.cfg.local_host_id
                        || st.peer_online.get(&mh).copied().unwrap_or(false);
                    if online {
                        st.active_cursor_host = Some(mh);
                    }
                }
            }
            st.cfg.save(&self.data_dir)?;
        }
        self.deskflow_reload_notify.notify_one();
        self.sync_peer_settings().await;
        self.dashboard_snapshot().await
    }

    pub async fn swap_input_provider(&self) -> Result<DashboardStateDto> {
        let next_id = {
            let st = self.state.read().await;
            let enabled_ids: Vec<String> = st
                .cfg
                .hosts
                .iter()
                .filter(|h| h.enabled)
                .map(|h| h.id.clone())
                .collect();
            if enabled_ids.len() >= 2 {
                let cur_idx = enabled_ids
                    .iter()
                    .position(|id| id == &st.cfg.input_provider_host_id)
                    .unwrap_or(0);
                Some(enabled_ids[(cur_idx + 1) % enabled_ids.len()].clone())
            } else {
                None
            }
        };
        if let Some(nid) = next_id {
            return self.set_input_provider(nid).await;
        }
        self.dashboard_snapshot().await
    }

    pub async fn toggle_plain_slide(&self) -> Result<DashboardStateDto> {
        {
            let mut st = self.state.write().await;
            if !st.cfg.deskflow_enabled {
                drop(st);
                return self.dashboard_snapshot().await;
            }
            st.cfg.allow_plain_slide_to_internal = !st.cfg.allow_plain_slide_to_internal;
            st.cfg.bump_shared_settings_rev();
            st.cfg.save(&self.data_dir)?;
        }
        self.deskflow_reload_notify.notify_one();
        self.sync_peer_settings().await;
        self.dashboard_snapshot().await
    }

    async fn promote_resolved_peer_ip(&self, peer_id: &str, resolved_ip: &str) {
        let changed = {
            let mut st = self.state.write().await;
            if let Some(h) = st.cfg.hosts.iter_mut().find(|h| h.id == peer_id) {
                if h.lan_ip != resolved_ip {
                    h.lan_ip = resolved_ip.to_string();
                    let _ = st.cfg.save(&self.data_dir);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if changed {
            self.deskflow_reload_notify.notify_one();
        }
    }

    async fn fetch_peer_status_with_mdns(
        &self,
        peer: &HostNode,
        port: u16,
        token: &str,
    ) -> Result<PeerStatusResponse> {
        {
            self.state
                .write()
                .await
                .reconcile_disconnected_cursor_host(Duration::from_secs(5));
        }
        match self.client.status(&peer.lan_ip, port, token).await {
            Ok(remote) => Ok(remote),
            Err(first_err) => {
                let candidates =
                    resolve_peer_mdns_ips(peer.mdns_fallback.as_deref(), &peer.lan_ip, port).await;
                for cand_ip in candidates {
                    if let Ok(remote) = self.client.status(&cand_ip, port, token).await {
                        if remote.host_id == peer.id {
                            self.promote_resolved_peer_ip(&peer.id, &cand_ip).await;
                            return Ok(remote);
                        }
                    }
                }
                Err(first_err)
            }
        }
    }

    async fn pair_peer_with_mdns(
        &self,
        peer: &HostNode,
        port: u16,
        token: &str,
        req: &PairPeerRequest,
    ) -> Result<PairPeerResponse> {
        match self.client.pair(&peer.lan_ip, port, token, req).await {
            Ok(resp) => Ok(resp),
            Err(first_err) => {
                let candidates =
                    resolve_peer_mdns_ips(peer.mdns_fallback.as_deref(), &peer.lan_ip, port).await;
                for cand_ip in candidates {
                    if let Ok(resp) = self.client.pair(&cand_ip, port, token, req).await {
                        if resp.host_id == peer.id {
                            self.promote_resolved_peer_ip(&peer.id, &cand_ip).await;
                            return Ok(resp);
                        }
                    }
                }
                Err(first_err)
            }
        }
    }

    async fn sync_peer_settings(&self) {
        let (cfg_snapshot, local_ip, port, token, peers, my_fp) = {
            let st = self.state.read().await;
            let local_ip = st
                .cfg
                .local_host()
                .map(|h| h.lan_ip.clone())
                .unwrap_or_default();
            let peers: Vec<HostNode> = st
                .cfg
                .hosts
                .iter()
                .filter(|h| h.enabled && h.id != st.cfg.local_host_id)
                .cloned()
                .collect();
            (
                st.cfg.clone(),
                local_ip,
                st.cfg.control_port,
                st.cfg.shared_token.clone(),
                peers,
                self.identity.v2_fingerprint.clone(),
            )
        };

        let req = PairPeerRequest::from_config(&cfg_snapshot, local_ip, my_fp);
        for p in peers {
            if let Ok(resp) = self
                .pair_peer_with_mdns(&p, port, &token, &req)
                .await
            {
                let mut st = self.state.write().await;
                st.peer_online.insert(p.id.clone(), true);
                if let Some(h) = st.cfg.hosts.iter_mut().find(|h| h.id == p.id) {
                    h.tls_fingerprint = Some(resp.tls_fingerprint);
                    if resp.mac_address.is_some() {
                        h.mac_address = resp.mac_address;
                    }
                }
                let _ = st.cfg.save(&self.data_dir);
            }
        }
    }

    pub async fn set_internal_alignment(&self, percent: u8) -> Result<DashboardStateDto> {
        {
            let mut st = self.state.write().await;
            st.cfg.set_internal_height_percent(percent);
            st.cfg.bump_shared_settings_rev();
            st.cfg.save(&self.data_dir)?;
        }
        self.deskflow_reload_notify.notify_one();
        self.sync_peer_settings().await;
        self.dashboard_snapshot().await
    }

    pub async fn pair_peers_now(&self) -> Result<DashboardStateDto> {
        self.sync_peer_settings().await;
        // Also refresh local hardware port probe and DDC active input/brightness/volume/preset on calibration
        let (probe, modes, ddc_obs, ddc_bri, ddc_vol, ddc_cp) = tokio::task::spawn_blocking(|| {
            let p = kvmddc_platform::probe_connected_monitor();
            let m = map_display_modes(kvmddc_platform::list_display_modes(true));
            let obs = kvmddc_platform::read_input_source().ok();
            let bri = kvmddc_platform::read_brightness().ok();
            let vol = kvmddc_platform::read_volume().ok();
            let cp = kvmddc_platform::read_color_preset().ok();
            (p, m, obs, bri, vol, cp)
        })
        .await
        .unwrap_or((
            ConnectedMonitorProbe::default(),
            Vec::new(),
            None,
            None,
            None,
            None,
        ));

        {
            let mut st = self.state.write().await;
            st.external_display_online = probe.connected || ddc_obs.is_some();
            if let Some(det_in) = probe.detected_input {
                let my_id = st.cfg.local_host_id.clone();
                if st.cfg.reconcile_detected_port(&my_id, det_in, false) {
                    let _ = st.cfg.save(&self.data_dir);
                }
            }
            st.local_probe = probe;
            if !modes.is_empty() {
                st.local_display_modes = modes;
            }
            if let Some(obs) = ddc_obs {
                st.observed_monitor_input = Some(obs);
            }
            if let Some(b) = ddc_bri {
                st.current_brightness = Some(b);
            }
            if let Some(v) = ddc_vol {
                st.current_volume = Some(v);
            }
            if let Some(cp) = ddc_cp {
                st.current_color_preset = Some(cp);
            }
            st.last_error = None;
        }

        self.deskflow_reload_notify.notify_one();
        self.dashboard_snapshot().await
    }

    pub async fn set_host_port_override(
        &self,
        host_id: String,
        monitor_input: Option<u8>,
    ) -> Result<DashboardStateDto> {
        {
            let mut st = self.state.write().await;
            if st.cfg.set_host_port_override(&host_id, monitor_input) {
                st.cfg.save(&self.data_dir)?;
            }
        }
        self.dashboard_snapshot().await
    }

    pub async fn open_accessibility_settings(&self) -> Result<DashboardStateDto> {
        let trusted = tokio::task::spawn_blocking(kvmddc_platform::prompt_and_open_accessibility_settings)
            .await
            .unwrap_or(Ok(false))
            .unwrap_or(false);
        {
            let mut st = self.state.write().await;
            if !st.accessibility_trusted && trusted {
                self.deskflow_reload_notify.notify_one();
            }
            st.accessibility_trusted = trusted;
        }
        self.dashboard_snapshot().await
    }

    pub async fn dashboard_snapshot(&self) -> Result<DashboardStateDto> {
        let _ = self.refresh_accessibility_trust().await;
        let st = self.state.read().await;
        let rt_row = self.db.get_runtime(&st.cfg.local_host_id)?;
        let cooldown_remaining_ms = StateDb::remaining_cooldown_ms(
            rt_row.and_then(|r| r.last_ddc_write_at),
            st.cfg.settle_ms,
            Utc::now(),
        );

        let monitor_model = st
            .local_probe
            .model_name
            .clone()
            .or_else(|| st.peer_monitor_model.clone())
            .unwrap_or_else(|| "27E2N6900RW".to_string());

        let cur_preset = st
            .current_color_preset
            .unwrap_or(st.cfg.preferred_color_preset);
        let cur_preset_label = color_preset_label(cur_preset).to_string();

        let hosts = st
            .cfg
            .hosts
            .iter()
            .map(|h| {
                let is_local = h.id == st.cfg.local_host_id;
                let online = if is_local {
                    true
                } else {
                    st.peer_online.get(&h.id).copied().unwrap_or(false)
                };
                let detected_live = if is_local {
                    st.local_probe.detected_input.is_some() && !h.manual_port_override
                } else {
                    st.peer_detected_inputs.contains_key(&h.id) && !h.manual_port_override
                };
                let port_label = if h.manual_port_override {
                    default_port_label_for_input(h.monitor_input).to_string()
                } else if is_local {
                    st.local_probe
                        .port_label
                        .clone()
                        .unwrap_or_else(|| default_port_label_for_input(h.monitor_input).to_string())
                } else {
                    st.peer_detected_ports
                        .get(&h.id)
                        .cloned()
                        .unwrap_or_else(|| default_port_label_for_input(h.monitor_input).to_string())
                };
                let live_mode_summary = if is_local {
                    st.local_display_modes
                        .iter()
                        .find(|m| m.active)
                        .map(|m| format!("{}×{} · {}Hz", m.width, m.height, m.hz))
                        .or_else(|| st.local_probe.mode_summary())
                } else {
                    st.peer_detected_modes.get(&h.id).cloned()
                };
                let profile_summary = live_mode_summary.clone().unwrap_or_else(|| {
                    format!(
                        "{}×{} · {}Hz",
                        h.display_profile.preferred_width,
                        h.display_profile.preferred_height,
                        h.display_profile.preferred_hz
                    )
                });
                let host_cp = h.preferred_color_preset.unwrap_or(cur_preset);
                let display_modes = if is_local {
                    if st.local_display_modes.is_empty() {
                        fallback_display_modes(
                            h.platform,
                            h.display_profile.preferred_width,
                            h.display_profile.preferred_height,
                            h.display_profile.preferred_hz,
                        )
                    } else {
                        st.local_display_modes.clone()
                    }
                } else {
                    st.peer_display_modes
                        .get(&h.id)
                        .filter(|v| !v.is_empty())
                        .cloned()
                        .unwrap_or_else(|| {
                            fallback_display_modes(
                                h.platform,
                                h.display_profile.preferred_width,
                                h.display_profile.preferred_height,
                                h.display_profile.preferred_hz,
                            )
                        })
                };
                DashboardHostDto {
                    id: h.id.clone(),
                    display_name: h.display_name.clone(),
                    platform: h.platform,
                    lan_ip: h.lan_ip.clone(),
                    monitor_input: h.monitor_input,
                    port_label,
                    detected_live,
                    manual_port_override: h.manual_port_override,
                    hotkey_slot: h.hotkey_slot,
                    enabled: h.enabled,
                    online,
                    profile_summary,
                    live_mode_summary,
                    internal_height_percent: h.display_profile.internal_height_percent,
                    preferred_brightness: h.preferred_brightness,
                    preferred_color_preset: h.preferred_color_preset,
                    color_preset_label: color_preset_label(host_cp).to_string(),
                    display_modes,
                }
            })
            .collect();

        let recent_requests = self
            .db
            .list_recent_requests(20)?
            .into_iter()
            .map(|r| DashboardRequestDto {
                id: r.id,
                to_host: r.to_host,
                target_input: r.target_input,
                observed_input: r.observed_input,
                source: r.source,
                status: r.status.as_str().to_string(),
                attempts: r.attempts,
                time_hms: r
                    .created_at
                    .with_timezone(&Local)
                    .format("%H:%M:%S")
                    .to_string(),
            })
            .collect();

        Ok(DashboardStateDto {
            local_host_id: st.cfg.local_host_id.clone(),
            input_provider_host_id: st.cfg.input_provider_host_id.clone(),
            primary_ddc_controller_id: st.cfg.primary_ddc_controller_id.clone(),
            deskflow_enabled: st.cfg.deskflow_enabled,
            ctrl_edge_switch_enabled: st.cfg.ctrl_edge_switch_enabled,
            allow_plain_slide_to_internal: st.cfg.allow_plain_slide_to_internal,
            unify_cross_os_modifiers: st.cfg.unify_cross_os_modifiers,
            auto_switch_input_provider_on_hotplug: st.cfg.auto_switch_input_provider_on_hotplug,
            observed_monitor_input: st.observed_monitor_input,
            preferred_brightness: st.cfg.preferred_brightness,
            current_brightness: st.current_brightness,
            preferred_volume: st.cfg.preferred_volume,
            current_volume: st.current_volume,
            audio_muted: st.cfg.audio_muted,
            current_color_preset: cur_preset,
            color_preset_label: cur_preset_label,
            last_transfer_summary: st.last_transfer_summary.clone(),
            deskflow_running: st.deskflow_running,
            deskflow_connected: st.deskflow_connected,
            accessibility_trusted: st.accessibility_trusted,
            display_sleeping: st.display_sleeping,
            external_display_online: st.external_display_online,
            monitor_model,
            edge_locked: st.is_deskflow_session_locked(),
            clamshell_closed: st.clamshell_closed,
            plain_slide_active: st.effective_plain_slide_peer_id().is_some(),
            secure_input_active: st.secure_input_active,
            cooldown_remaining_ms,
            last_error: st.last_error.clone(),
            hosts,
            recent_requests,
        })
    }

    pub async fn dismiss_last_error(&self) -> Result<DashboardStateDto> {
        {
            let mut st = self.state.write().await;
            st.last_error = None;
        }
        self.dashboard_snapshot().await
    }
}

struct TransferEntry {
    abs_path: PathBuf,
    rel_path: String,
    display_leaf: String,
}

fn spawn_deskflow_output_reader<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    reader: R,
    rt: Arc<AppRuntime>,
    tap_flag: Arc<std::sync::atomic::AtomicBool>,
    shape_flag: Arc<std::sync::atomic::AtomicBool>,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.contains("failed to create quartz event tap") {
                tap_flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if line.contains("failed to initialize computer shape") {
                shape_flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            rt.ingest_deskflow_log(&line).await;
        }
    });
}

fn map_display_modes(list: Vec<DisplayModeOption>) -> Vec<DisplayModeDto> {
    list.into_iter()
        .map(|m| DisplayModeDto {
            mode_id: m.mode_id,
            width: m.width,
            height: m.height,
            pixel_width: m.pixel_width,
            pixel_height: m.pixel_height,
            hz: m.hz,
            hidpi: m.hidpi,
            active: m.current,
            label: m.label,
        })
        .collect()
}

fn fallback_display_modes(
    platform: HostPlatform,
    pref_w: u32,
    pref_h: u32,
    pref_hz: u32,
) -> Vec<DisplayModeDto> {
    let presets: &[(u32, u32, u32, bool)] = match platform {
        HostPlatform::Macos => &[
            (2560, 1440, 75, true),
            (3200, 1800, 75, true),
            (3840, 2160, 75, true),
            (5120, 2880, 75, false),
            (1920, 1080, 75, true),
        ],
        HostPlatform::Windows => &[
            (5120, 2880, 75, false),
            (3840, 2160, 75, false),
            (2560, 1440, 75, false),
            (2560, 1440, 300, false),
            (1920, 1080, 75, false),
        ],
    };
    let mut out = Vec::new();
    let mut matched = false;
    for (idx, &(w, h, hz, hidpi)) in presets.iter().enumerate() {
        let active = !matched && w == pref_w && h == pref_h && (pref_hz == 0 || hz == pref_hz);
        if active {
            matched = true;
        }
        let suffix = if hidpi { " · HiDPI" } else { "" };
        out.push(DisplayModeDto {
            mode_id: idx as i32,
            width: w,
            height: h,
            pixel_width: if hidpi { w * 2 } else { w },
            pixel_height: if hidpi { h * 2 } else { h },
            hz,
            hidpi,
            active,
            label: format!("{}×{} · {}Hz{}", w, h, hz, suffix),
        });
    }
    if !matched {
        if let Some(first) = out.first_mut() {
            first.active = true;
        }
    }
    out
}

fn active_display_host_mut<'a>(
    cfg: &'a mut AppConfig,
    observed_input: Option<u8>,
    active_cursor: Option<&str>,
) -> Option<&'a mut HostNode> {
    if let Some(inp) = observed_input {
        if let Some(idx) = cfg
            .hosts
            .iter()
            .position(|h| h.enabled && h.monitor_input == inp)
        {
            return cfg.hosts.get_mut(idx);
        }
    }
    if let Some(cid) = active_cursor {
        if let Some(idx) = cfg.hosts.iter().position(|h| h.enabled && h.id == cid) {
            return cfg.hosts.get_mut(idx);
        }
    }
    let lid = cfg.local_host_id.clone();
    cfg.hosts.iter_mut().find(|h| h.id == lid)
}

fn default_receive_dir(fallback_data_dir: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let trimmed = profile.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("Downloads").join("Kvmddc");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("Downloads").join("Kvmddc");
        }
    }
    fallback_data_dir.join("Transfers")
}

fn collect_transfer_entries(raw_paths: &[String]) -> Result<Vec<TransferEntry>> {
    const MAX_FILES: usize = 200;
    const MAX_SINGLE_BYTES: u64 = 512 * 1024 * 1024;
    let mut out = Vec::new();

    for raw in raw_paths {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let root = PathBuf::from(trimmed);
        let meta = std::fs::symlink_metadata(&root)
            .with_context(|| format!("读取待传路径失败: {}", root.display()))?;
        if meta.file_type().is_symlink() {
            continue;
        }
        let root_name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .context("无法解析待传文件名")?;

        if meta.is_file() {
            ensure!(
                meta.len() <= MAX_SINGLE_BYTES,
                "文件超过 512MB 单文件直传上限: {root_name}"
            );
            out.push(TransferEntry {
                abs_path: root,
                rel_path: root_name.clone(),
                display_leaf: root_name,
            });
            ensure!(out.len() <= MAX_FILES, "单次拖拽文件数上限为 {MAX_FILES} 个");
        } else if meta.is_dir() {
            walk_dir_entries(&root, &root_name, 0, MAX_FILES, MAX_SINGLE_BYTES, &mut out)?;
        }
    }

    Ok(out)
}

fn walk_dir_entries(
    dir: &Path,
    rel_prefix: &str,
    depth: usize,
    max_files: usize,
    max_bytes: u64,
    out: &mut Vec<TransferEntry>,
) -> Result<()> {
    if depth > 8 {
        return Ok(());
    }
    for item in std::fs::read_dir(dir)? {
        let entry = item?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".DS_Store" || name == "Thumbs.db" {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        let next_rel = format!("{rel_prefix}/{name}");
        let p = entry.path();
        if ft.is_file() {
            let len = entry.metadata()?.len();
            ensure!(
                len <= max_bytes,
                "文件夹内单文件超过 512MB 上限: {next_rel}"
            );
            out.push(TransferEntry {
                abs_path: p,
                rel_path: next_rel,
                display_leaf: rel_prefix.to_string(),
            });
            ensure!(out.len() <= max_files, "单次拖拽文件数上限为 {max_files} 个");
        } else if ft.is_dir() {
            walk_dir_entries(&p, &next_rel, depth + 1, max_files, max_bytes, out)?;
        }
    }
    Ok(())
}

fn resolve_non_overwriting_path(base_dir: &Path, rel_path: &Path, incoming_sha: &str) -> PathBuf {
    let candidate = base_dir.join(rel_path);
    if !candidate.exists() {
        return candidate;
    }
    if let Ok(existing_bytes) = std::fs::read(&candidate) {
        if sha256_hex_bytes(&existing_bytes) == incoming_sha {
            return candidate;
        }
    }
    let parent = candidate.parent().unwrap_or(base_dir);
    let stem = candidate
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let ext = candidate
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for idx in 2..=999u32 {
        let alt = parent.join(format!("{stem}-{idx}{ext}"));
        if !alt.exists() {
            return alt;
        }
    }
    parent.join(format!("{stem}-{}{ext}", Utc::now().timestamp()))
}

impl ControlPlaneHandler for AppRuntime {
    fn shared_token(&self) -> String {
        self.shared_token.clone()
    }

    fn get_status(&self) -> BoxFuture<'_, Result<PeerStatusResponse>> {
        Box::pin(async move {
            let st = self.state.read().await;
            let active_switch = self
                .db
                .active_executing_request(&st.cfg.local_host_id)
                .unwrap_or(None);
            Ok(PeerStatusResponse {
                host_id: st.cfg.local_host_id.clone(),
                platform: HostPlatform::current(),
                is_input_provider: st.cfg.is_input_provider(),
                is_ddc_controller: st.cfg.is_ddc_controller(),
                deskflow_running: st.deskflow_running,
                deskflow_connected: st.deskflow_connected,
                active_cursor_host: st.active_cursor_host.clone(),
                observed_monitor_input: st.observed_monitor_input,
                current_brightness: st.current_brightness,
                edge_locked: st.edge_locked,
                secure_input_active: st.secure_input_active,
                tls_fingerprint: self.identity.v2_fingerprint.clone(),
                active_switch,
                detected_monitor_input: st.local_probe.detected_input,
                detected_port_label: st.local_probe.port_label.clone(),
                detected_mode_summary: st.local_probe.mode_summary(),
                detected_monitor_model: st.local_probe.model_name.clone(),
                display_sleeping: st.display_sleeping,
                current_volume: st.current_volume.or(Some(st.cfg.preferred_volume)),
                audio_muted: Some(st.cfg.audio_muted),
                current_color_preset: st
                    .current_color_preset
                    .or(Some(st.cfg.preferred_color_preset)),
                display_modes: st.local_display_modes.clone(),
                clamshell_closed: st.clamshell_closed,
                shared_settings_rev: st.cfg.shared_settings_rev,
                input_provider_host_id: Some(st.cfg.input_provider_host_id.clone()),
                primary_ddc_controller_id: Some(st.cfg.primary_ddc_controller_id.clone()),
                deskflow_enabled: Some(st.cfg.deskflow_enabled),
                ctrl_edge_switch_enabled: Some(st.cfg.ctrl_edge_switch_enabled),
                allow_plain_slide_to_internal: Some(st.cfg.allow_plain_slide_to_internal),
                unify_cross_os_modifiers: Some(st.cfg.unify_cross_os_modifiers),
                auto_switch_input_provider_on_hotplug: Some(
                    st.cfg.auto_switch_input_provider_on_hotplug,
                ),
                internal_height_percent: Some(st.cfg.internal_height_percent()),
            })
        })
    }

    fn handle_switch(&self, req: RemoteSwitchRequest) -> BoxFuture<'_, Result<ActionAck>> {
        Box::pin(async move {
            let (
                local_id,
                settle_ms,
                is_ctrl,
                is_provider,
                slot_opt,
                was_sleeping,
                prev_observed_input,
                local_ext_online,
            ) = {
                let st = self.state.read().await;
                let slot = st
                    .cfg
                    .hosts
                    .iter()
                    .find(|h| h.id == req.to_host)
                    .map(|h| h.hotkey_slot);
                (
                    st.cfg.local_host_id.clone(),
                    st.cfg.settle_ms,
                    st.cfg.is_ddc_controller(),
                    st.cfg.is_input_provider(),
                    slot,
                    st.display_sleeping,
                    st.observed_monitor_input,
                    st.external_display_online,
                )
            };

            CLIENT_REFOCUS_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let reload_df = {
                let mut st = self.state.write().await;
                st.last_switch_activity_at = Instant::now();
                st.observed_monitor_input = Some(req.target_input);
                if req.sync_input_focus || req.to_host == local_id {
                    st.active_cursor_host = Some(req.to_host.clone());
                }
                let confine = st.should_confine_builtin_display();
                st.applied_confined_builtin = confine;
                kvmddc_platform::set_builtin_display_confined(confine);
                sync_confine_builtin_flag(&self.data_dir, confine);
                st.should_reload_deskflow_links()
            };
            if reload_df {
                self.deskflow_reload_notify.notify_one();
            }

            if req.sync_input_focus && is_provider {
                if let Some(slot) = slot_opt {
                    self.focus_deskflow_on_host(&req.to_host, slot, &[250, 800, 1800, 3000])
                        .await;
                }
            }

            if !is_ctrl {
                let target_in = req.target_input;
                let write_res = tokio::task::spawn_blocking(move || {
                    if kvmddc_platform::read_input_source().ok() == Some(target_in) {
                        return Ok(());
                    }
                    kvmddc_platform::write_input_source(target_in)
                })
                .await
                .unwrap_or_else(|e| Err(anyhow::anyhow!("{e}")));
                return match write_res {
                    Ok(()) => Ok(ActionAck {
                        ok: true,
                        status: "peer_written".to_string(),
                        message: "已通过持屏端 DDC 写入输入源".to_string(),
                    }),
                    Err(e) => Err(e),
                };
            }

            if !was_sleeping {
                let live_in = tokio::task::spawn_blocking(kvmddc_platform::read_input_source)
                    .await
                    .ok()
                    .and_then(|r| r.ok());
                let already_active = match live_in {
                    Some(cur) => cur == req.target_input,
                    None => {
                        if req.to_host == local_id {
                            local_ext_online && prev_observed_input == Some(req.target_input)
                        } else {
                            !local_ext_online && prev_observed_input == Some(req.target_input)
                        }
                    }
                };
                if already_active {
                    return Ok(ActionAck {
                        ok: true,
                        status: "already_active".to_string(),
                        message: "当前已是该设备画面，无需重复切屏".to_string(),
                    });
                }
            }

            let out = self.db.enqueue_switch(
                NewSwitchRequest {
                    request_key: req.request_key,
                    source: req.source,
                    controller: local_id,
                    from_host: req.from_host,
                    to_host: Some(req.to_host),
                    target_input: req.target_input,
                },
                settle_ms,
                Utc::now(),
            )?;
            self.ddc_notify.notify_one();

            let status = match out {
                kvmddc_core::db::EnqueueOutcome::DuplicateIgnored(r) => r.status.as_str().to_string(),
                kvmddc_core::db::EnqueueOutcome::ReadyToWrite(r) => r.status.as_str().to_string(),
                kvmddc_core::db::EnqueueOutcome::QueuedPending { row, .. } => {
                    row.status.as_str().to_string()
                }
            };
            Ok(ActionAck {
                ok: true,
                status,
                message: "已进入切屏队列".to_string(),
            })
        })
    }

    fn handle_focus_input(&self, req: FocusInputRequest) -> BoxFuture<'_, Result<ActionAck>> {
        Box::pin(async move {
            CLIENT_REFOCUS_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            {
                let mut st = self.state.write().await;
                st.last_switch_activity_at = Instant::now();
                st.active_cursor_host = Some(req.target_host_id.clone());
                if let Some(th_in) = st
                    .cfg
                    .hosts
                    .iter()
                    .find(|h| h.enabled && h.id == req.target_host_id)
                    .filter(|h| {
                        h.display_profile.has_internal_screen
                            && st.is_host_clamshell_closed(&h.id)
                    })
                    .map(|h| h.monitor_input)
                {
                    st.observed_monitor_input = Some(th_in);
                }
            }
            self.focus_deskflow_on_host(&req.target_host_id, req.hotkey_slot, &[250, 900])
                .await;
            Ok(ActionAck {
                ok: true,
                status: "focused".to_string(),
                message: "已切换键鼠焦点".to_string(),
            })
        })
    }

    fn handle_brightness(&self, req: BrightnessRequest) -> BoxFuture<'_, Result<ActionAck>> {
        Box::pin(async move {
            self.set_brightness_global(req.brightness).await?;
            Ok(ActionAck {
                ok: true,
                status: "applied".to_string(),
                message: format!("亮度已设为 {}%", req.brightness),
            })
        })
    }

    fn handle_volume(&self, req: VolumeRequest) -> BoxFuture<'_, Result<ActionAck>> {
        Box::pin(async move {
            let _ = self.set_volume_global(req.volume, req.muted).await?;
            Ok(ActionAck {
                ok: true,
                status: "applied".to_string(),
                message: "显示器音量已更新".to_string(),
            })
        })
    }

    fn handle_color_preset(&self, req: ColorPresetRequest) -> BoxFuture<'_, Result<ActionAck>> {
        Box::pin(async move {
            self.set_color_preset_global(req.preset).await?;
            Ok(ActionAck {
                ok: true,
                status: "applied".to_string(),
                message: format!("色温已设为 {}", color_preset_label(req.preset)),
            })
        })
    }

    fn handle_display_mode(&self, req: DisplayModeRequest) -> BoxFuture<'_, Result<ActionAck>> {
        Box::pin(async move {
            let local_id = self.state.read().await.cfg.local_host_id.clone();
            let _ = self
                .select_display_mode(
                    Some(local_id),
                    req.mode_id,
                    req.width,
                    req.height,
                    req.hz,
                    req.hidpi,
                )
                .await?;
            let label = {
                let st = self.state.read().await;
                st.local_display_modes
                    .iter()
                    .find(|m| m.active)
                    .map(|m| m.label.clone())
                    .unwrap_or_else(|| format!("{}×{} · {}Hz", req.width, req.height, req.hz))
            };
            Ok(ActionAck {
                ok: true,
                status: "applied".to_string(),
                message: label,
            })
        })
    }

    fn handle_display_sleep(&self) -> BoxFuture<'_, Result<ActionAck>> {
        Box::pin(async move {
            self.enter_local_standby().await;
            Ok(ActionAck {
                ok: true,
                status: "sleeping".to_string(),
                message: "已进入不断电熄屏待机".to_string(),
            })
        })
    }

    fn handle_wake(&self) -> BoxFuture<'_, Result<ActionAck>> {
        Box::pin(async move {
            let awake = self.exit_local_standby().await;
            Ok(ActionAck {
                ok: awake,
                status: if awake { "awake" } else { "timeout" }.to_string(),
                message: "已执行亮屏唤醒".to_string(),
            })
        })
    }

    fn handle_pair(&self, req: PairPeerRequest) -> BoxFuture<'_, Result<PairPeerResponse>> {
        Box::pin(async move {
            validate_pure_lan_ip(&req.lan_ip)?;
            let incoming_shared = req.to_shared_settings();
            let (local_id, reload_df, hide_picker, push_back) = {
                let mut st = self.state.write().await;
                st.peer_online.insert(req.host_id.clone(), true);
                let mut dirty = false;
                let mut ip_changed = false;
                if let Some(h) = st.cfg.hosts.iter_mut().find(|h| h.id == req.host_id) {
                    if h.lan_ip != req.lan_ip {
                        h.lan_ip = req.lan_ip;
                        dirty = true;
                        ip_changed = true;
                    }
                    if h.tls_fingerprint.as_deref() != Some(req.tls_fingerprint.as_str()) {
                        h.tls_fingerprint = Some(req.tls_fingerprint);
                        dirty = true;
                    }
                    if req.mac_address.is_some() && h.mac_address != req.mac_address {
                        h.mac_address = req.mac_address;
                        dirty = true;
                    }
                }
                let (sync_out, disabled_now) =
                    st.apply_shared_reconcile(&incoming_shared, &req.host_id);
                if sync_out.changed {
                    dirty = true;
                }
                if dirty {
                    st.cfg.save(&self.data_dir)?;
                }
                (
                    st.cfg.local_host_id.clone(),
                    ip_changed
                        || sync_out.deskflow_reload_needed
                        || st.should_reload_deskflow_links(),
                    disabled_now,
                    sync_out.push_back_to_source,
                )
            };
            if hide_picker {
                self.hide_edge_picker().await;
            }
            if reload_df {
                self.deskflow_reload_notify.notify_one();
            }
            if push_back {
                self.sync_peer_settings().await;
            }
            Ok(PairPeerResponse {
                host_id: local_id,
                tls_fingerprint: self.identity.v2_fingerprint.clone(),
                mac_address: None,
            })
        })
    }

    fn handle_receive_file(
        &self,
        rel_path: PathBuf,
        data: Bytes,
    ) -> BoxFuture<'_, Result<ActionAck>> {
        Box::pin(async move {
            let base_dir = default_receive_dir(&self.data_dir);
            let incoming_sha = sha256_hex_bytes(&data);
            let payload = data.to_vec();
            let rel_clone = rel_path.clone();

            let saved_path = tokio::task::spawn_blocking(move || -> Result<PathBuf> {
                let dest = resolve_non_overwriting_path(&base_dir, &rel_clone, &incoming_sha);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let tmp_name = format!(
                    ".{}.kvmddc-part",
                    dest.file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_else(|| "transfer".to_string())
                );
                let tmp_path = dest.with_file_name(tmp_name);
                std::fs::write(&tmp_path, &payload)?;
                std::fs::rename(&tmp_path, &dest)?;
                Ok(dest)
            })
            .await
            .context("文件落盘线程异常")??;

            let leaf = saved_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "文件".to_string());
            let summary = format!("已收文件 · {leaf}");
            self.state.write().await.last_transfer_summary = Some(summary.clone());
            self.emit_hud("transfer", &summary, 100, false).await;

            Ok(ActionAck {
                ok: true,
                status: "saved".to_string(),
                message: saved_path.to_string_lossy().to_string(),
            })
        })
    }

    fn handle_receive_clipboard_png(&self, png_data: Bytes) -> BoxFuture<'_, Result<ActionAck>> {
        Box::pin(async move {
            let sha = sha256_hex_bytes(&png_data);
            {
                let mut st = self.state.write().await;
                if st.last_clipboard_png_sha.as_deref() == Some(sha.as_str()) {
                    return Ok(ActionAck {
                        ok: true,
                        status: "unchanged".to_string(),
                        message: "截图已在剪贴板中".to_string(),
                    });
                }
                st.last_clipboard_png_sha = Some(sha);
            }
            let buf = png_data.to_vec();
            tokio::task::spawn_blocking(move || kvmddc_platform::write_clipboard_png(&buf))
                .await
                .context("写入系统剪贴板线程异常")??;
            Ok(ActionAck {
                ok: true,
                status: "synced".to_string(),
                message: "已同步剪贴板截图".to_string(),
            })
        })
    }
}

impl LiveState {
    /// Clears stale `active_cursor_host` pointing to a disconnected/offline peer on the active
    /// input provider after a switch grace window so Deskflow and UI focus never strand.
    fn reconcile_disconnected_cursor_host(&mut self, grace: Duration) {
        if !self.cfg.is_input_provider() || self.last_switch_activity_at.elapsed() < grace {
            return;
        }
        let Some(cur) = self.active_cursor_host.as_deref() else {
            return;
        };
        if cur == self.cfg.local_host_id {
            return;
        }
        let peer_online = self.peer_online.get(cur).copied().unwrap_or(false);
        let peer_locked = self.peer_edge_locked.get(cur).copied().unwrap_or(false);
        let deskflow_link_up = !self.cfg.deskflow_enabled || self.deskflow_connected;
        if !peer_online || peer_locked || !deskflow_link_up {
            self.active_cursor_host = Some(self.cfg.local_host_id.clone());
        }
    }

    /// Returns `(host_id, hotkey_slot)` of the remote host currently displayed on the 27" monitor
    /// when a Deskflow reset back to `local_host_id` is involuntary (caused by a peer resolution /
    /// DP-wake shape change, or occurring while `active_cursor_host` was on a remote peer and no
    /// plain slide link exists during a switch window).
    ///
    /// Crucial invariant: if `self.active_cursor_host` is ALREADY `local_host_id`, switching to
    /// `local_host_id` was intentional and MUST NEVER bounce back to a peer!
    fn involuntary_local_reset_escape_target(&self) -> Option<(String, u8)> {
        if !self.cfg.is_input_provider()
            || self.active_cursor_host.as_deref() == Some(self.cfg.local_host_id.as_str())
        {
            return None;
        }
        let inp = self.observed_monitor_input?;
        let mh = self
            .cfg
            .hosts
            .iter()
            .find(|h| h.enabled && h.monitor_input == inp && h.id != self.cfg.local_host_id)?;
        let online = self.peer_online.get(&mh.id).copied().unwrap_or(false);
        let locked = self.peer_edge_locked.get(&mh.id).copied().unwrap_or(false);
        if !online || locked {
            return None;
        }
        let shape_changed_recently = LAST_PEER_SHAPE_CHANGE
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .is_some_and(|(hid, at)| hid == mh.id && at.elapsed() < Duration::from_millis(2200));
        let no_plain_slide_during_switch = self.effective_plain_slide_peer_id().is_none()
            && self.last_switch_activity_at.elapsed() < Duration::from_millis(4000);
        if shape_changed_recently || no_plain_slide_during_switch {
            Some((mh.id.clone(), mh.hotkey_slot))
        } else {
            None
        }
    }

    /// Update Deskflow connection/focus state from one parsed log event and optionally return
    /// `(host_id, hotkey_slot)` to refocus an already-visible remote screen after reconnect.
    pub(crate) fn apply_deskflow_log_event(
        &mut self,
        ev: DeskflowLogEvent,
    ) -> Option<(String, u8)> {
        match ev {
            DeskflowLogEvent::PeerShapeChanged { host_id } => {
                if self.cfg.is_input_provider() {
                    if let Ok(mut guard) = LAST_PEER_SHAPE_CHANGE.lock() {
                        *guard = Some((host_id.clone(), Instant::now()));
                    }
                    if let Some(h) = self
                        .cfg
                        .hosts
                        .iter()
                        .find(|h| h.enabled && h.id == host_id && h.id != self.cfg.local_host_id)
                        .cloned()
                    {
                        if self.observed_monitor_input == Some(h.monitor_input) {
                            self.active_cursor_host = Some(host_id.clone());
                            self.last_switch_activity_at = Instant::now();
                            return Some((host_id, h.hotkey_slot));
                        }
                    }
                }
                None
            }
            DeskflowLogEvent::Switched { to, .. } => {
                self.deskflow_screen_host = Some(to.clone());
                if to == self.cfg.local_host_id {
                    if let Some((esc_id, esc_slot)) = self.involuntary_local_reset_escape_target() {
                        self.active_cursor_host = Some(esc_id.clone());
                        return Some((esc_id, esc_slot));
                    }
                }
                let matched_followup = FOCUS_FOLLOWUP_TARGET
                    .lock()
                    .ok()
                    .is_some_and(|mut g| {
                        if g.as_deref() == Some(to.as_str()) {
                            *g = None;
                            true
                        } else {
                            false
                        }
                    });
                if matched_followup || self.active_cursor_host.as_deref() != Some(to.as_str()) {
                    FOCUS_FOLLOWUP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                self.active_cursor_host = Some(to);
                None
            }
            DeskflowLogEvent::ClientConnected => self.on_deskflow_client_connected(None),
            DeskflowLogEvent::PeerClientConnected { host_id } => {
                self.on_deskflow_client_connected(Some(host_id))
            }
            DeskflowLogEvent::PeerClientDisconnected { host_id } => {
                if self.deskflow_screen_host.as_deref() == Some(host_id.as_str()) {
                    self.deskflow_screen_host = Some(self.cfg.local_host_id.clone());
                }
                if self.cfg.is_input_provider() {
                    if !self.has_other_online_peer(&host_id) {
                        self.deskflow_connected = false;
                    }
                    if self.active_cursor_host.as_deref() == Some(host_id.as_str()) {
                        return self.recover_cursor_after_invisible_or_lost_target(&host_id);
                    }
                } else {
                    self.deskflow_connected = false;
                }
                None
            }
            DeskflowLogEvent::ClientDisconnected => {
                if !self.cfg.is_input_provider() {
                    self.deskflow_connected = false;
                } else if !self.has_other_online_peer("") {
                    self.deskflow_connected = false;
                    self.deskflow_screen_host = Some(self.cfg.local_host_id.clone());
                    if self.active_cursor_host.as_deref() != Some(self.cfg.local_host_id.as_str())
                    {
                        self.active_cursor_host = Some(self.cfg.local_host_id.clone());
                    }
                }
                None
            }
            DeskflowLogEvent::EnteringScreen => {
                let local_id = self.cfg.local_host_id.clone();
                self.deskflow_screen_host = Some(local_id.clone());
                if let Some((esc_id, esc_slot)) = self.involuntary_local_reset_escape_target() {
                    self.active_cursor_host = Some(esc_id.clone());
                    return Some((esc_id, esc_slot));
                }
                let matched_followup = FOCUS_FOLLOWUP_TARGET
                    .lock()
                    .ok()
                    .is_some_and(|mut g| {
                        if g.as_deref() == Some(local_id.as_str()) {
                            *g = None;
                            true
                        } else {
                            false
                        }
                    });
                if matched_followup || self.active_cursor_host.as_deref() != Some(local_id.as_str())
                {
                    FOCUS_FOLLOWUP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                self.active_cursor_host = Some(local_id);
                None
            }
            DeskflowLogEvent::LeavingScreen => None,
        }
    }

    fn has_other_online_peer(&self, excluded_host_id: &str) -> bool {
        self.cfg.hosts.iter().any(|h| {
            h.enabled
                && h.id != self.cfg.local_host_id
                && h.id != excluded_host_id
                && self.peer_online.get(&h.id).copied().unwrap_or(false)
        })
    }

    fn recover_cursor_after_invisible_or_lost_target(
        &mut self,
        blocked_target_id: &str,
    ) -> Option<(String, u8)> {
        let local_id = self.cfg.local_host_id.clone();
        let local_input = self.cfg.local_host().ok().map(|lh| lh.monitor_input);
        let local_closed_invisible = self
            .cfg
            .local_host()
            .ok()
            .map(|lh| {
                lh.display_profile.has_internal_screen
                    && self.clamshell_closed
                    && self.observed_monitor_input != Some(lh.monitor_input)
            })
            .unwrap_or(false);

        if !local_closed_invisible && self.observed_monitor_input == local_input {
            self.active_cursor_host = Some(local_id);
            return None;
        }
        let candidates = [
            Some(blocked_target_id),
            local_closed_invisible.then_some(local_id.as_str()),
        ];
        for candidate in candidates.into_iter().flatten() {
            if let Some((esc_id, esc_slot)) = self.monitor_escape_target_for_closed_laptop(candidate) {
                if esc_id != blocked_target_id {
                    self.active_cursor_host = Some(esc_id.clone());
                    return (esc_id != local_id).then_some((esc_id, esc_slot));
                }
            }
        }
        self.active_cursor_host = Some(local_id);
        None
    }

    fn on_deskflow_client_connected(
        &mut self,
        connected_peer: Option<String>,
    ) -> Option<(String, u8)> {
        self.deskflow_connected = true;
        if !self.cfg.is_input_provider() {
            return None;
        }
        if let Some(ref peer_id) = connected_peer {
            if let Some(h) = self
                .cfg
                .hosts
                .iter()
                .find(|h| h.enabled && h.id == *peer_id && h.id != self.cfg.local_host_id)
                .cloned()
            {
                if self.observed_monitor_input == Some(h.monitor_input) {
                    self.active_cursor_host = Some(peer_id.clone());
                    self.last_switch_activity_at = Instant::now();
                    return Some((peer_id.clone(), h.hotkey_slot));
                }
            }
        }
        let tid = self.active_cursor_host.clone()?;
        if tid == self.cfg.local_host_id {
            return None;
        }
        if let Some(ref peer_id) = connected_peer {
            if peer_id != &tid {
                return None;
            }
        }
        let h = self
            .cfg
            .hosts
            .iter()
            .find(|h| h.enabled && h.id == tid)
            .cloned()?;

        let target_on_shared_monitor = self.observed_monitor_input == Some(h.monitor_input);
        let plain_slide_to_target = h.display_profile.has_internal_screen
            && !self.is_host_clamshell_closed(&tid)
            && self.effective_plain_slide_peer_id().as_deref() == Some(tid.as_str());

        if target_on_shared_monitor || plain_slide_to_target {
            return Some((tid, h.hotkey_slot));
        }

        let invisible_clamshell = h.display_profile.has_internal_screen
            && self.is_host_clamshell_closed(&tid)
            && self.observed_monitor_input != Some(h.monitor_input);
        if self.observed_monitor_input.is_some() || invisible_clamshell {
            return self.recover_cursor_after_invisible_or_lost_target(&tid);
        }

        Some((tid, h.hotkey_slot))
    }
}

const HID_HOTPLUG_POLL_INTERVAL: Duration = Duration::from_millis(450);
const HID_DISCOVERY_SETTLE: Duration = Duration::from_millis(700);
const HID_SELF_BOUNCE_GRACE: Duration = Duration::from_millis(2500);

/// Pure state machine tracking external USB/Bluetooth mouse & keyboard hotplug transitions.
///
/// First poll captures standing peripherals so existing host devices (such as a secondary Windows
/// mouse left plugged in) never trigger spurious provider handoffs. Newly arrived devices must
/// remain present across `HID_DISCOVERY_SETTLE` so multi-port hubs claim once together.
#[derive(Debug, Default)]
struct InputHotplugTracker {
    initialized: bool,
    known_keys: HashSet<String>,
    recent_removals: HashMap<String, Instant>,
    pending_arrivals: HashMap<String, Instant>,
    pending_untrusted_keys: HashSet<String>,
}

impl InputHotplugTracker {
    fn new() -> Self {
        Self::default()
    }

    /// Evaluates the latest external input device snapshot and returns newly settled devices when
    /// this host should claim `input_provider_host_id`.
    fn poll(
        &mut self,
        now: Instant,
        devices: &[ExternalInputDevice],
        eligible_to_claim: bool,
        ax_trusted: bool,
    ) -> Option<Vec<ExternalInputDevice>> {
        let mut current_map: HashMap<&str, &ExternalInputDevice> = HashMap::new();
        for d in devices {
            current_map.entry(d.device_key.as_str()).or_insert(d);
        }

        self.recent_removals.retain(|_, removed_at| {
            now.saturating_duration_since(*removed_at) < HID_SELF_BOUNCE_GRACE
        });

        if !self.initialized {
            self.initialized = true;
            self.known_keys = current_map.keys().map(|k| (*k).to_string()).collect();
            return None;
        }

        let removed: Vec<String> = self
            .known_keys
            .iter()
            .filter(|k| !current_map.contains_key(k.as_str()))
            .cloned()
            .collect();
        for k in removed {
            self.known_keys.remove(&k);
            self.recent_removals.insert(k, now);
        }

        self.pending_arrivals
            .retain(|k, _| current_map.contains_key(k.as_str()));
        self.pending_untrusted_keys
            .retain(|k| current_map.contains_key(k.as_str()));

        for key in current_map.keys() {
            if self.known_keys.contains(*key) {
                continue;
            }
            // Brief unplug/replug bounce on the same host should not steal Deskflow back.
            if self.recent_removals.remove(*key).is_some() {
                self.known_keys.insert((*key).to_string());
                continue;
            }
            self.pending_arrivals
                .entry((*key).to_string())
                .or_insert(now);
        }

        if !eligible_to_claim {
            for k in self.pending_arrivals.keys() {
                self.known_keys.insert(k.clone());
            }
            self.pending_arrivals.clear();
            self.pending_untrusted_keys.clear();
            return None;
        }

        let settled = self
            .pending_arrivals
            .values()
            .any(|since| now.saturating_duration_since(*since) >= HID_DISCOVERY_SETTLE);
        if settled {
            let claimed: Vec<ExternalInputDevice> = devices
                .iter()
                .filter(|d| self.pending_arrivals.contains_key(&d.device_key))
                .cloned()
                .collect();
            for k in self.pending_arrivals.keys() {
                self.known_keys.insert(k.clone());
            }
            self.pending_arrivals.clear();
            if ax_trusted {
                self.pending_untrusted_keys.clear();
                if !claimed.is_empty() {
                    return Some(claimed);
                }
            } else {
                // Hold claim until macOS Accessibility TCC is granted so the cluster never switches
                // to a host that cannot run Deskflow Server.
                for d in claimed {
                    self.pending_untrusted_keys.insert(d.device_key);
                }
            }
            return None;
        }

        if ax_trusted && !self.pending_untrusted_keys.is_empty() {
            let deferred: Vec<ExternalInputDevice> = devices
                .iter()
                .filter(|d| self.pending_untrusted_keys.contains(&d.device_key))
                .cloned()
                .collect();
            self.pending_untrusted_keys.clear();
            if !deferred.is_empty() {
                return Some(deferred);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_local_ip_preserves_active_configured_nic_and_mdns_filters_pure_lan() {
        let active = vec![
            "192.168.2.207".to_string(),
            "192.168.2.36".to_string(),
            "100.64.0.42".to_string(),
        ];
        // On macOS (prefer_primary_nic = true), primary en0 (.207) beats transient USB-C hub NIC (.36)
        assert_eq!(
            select_preferred_local_pure_lan_ip(
                Some("192.168.2.36"),
                &active,
                Some("192.168.2.36"),
                true,
            )
            .as_deref(),
            Some("192.168.2.207")
        );
        // On Windows (prefer_primary_nic = false), active configured NIC (.36) is preserved
        assert_eq!(
            select_preferred_local_pure_lan_ip(
                Some("192.168.2.36"),
                &active,
                Some("192.168.2.207"),
                false,
            )
            .as_deref(),
            Some("192.168.2.36")
        );
        // When configured IP is no longer up on any interface, fall back to routed LAN IP
        assert_eq!(
            select_preferred_local_pure_lan_ip(
                Some("192.168.2.99"),
                &active,
                Some("192.168.2.207"),
                false,
            )
            .as_deref(),
            Some("192.168.2.207")
        );

        let addrs: Vec<std::net::SocketAddr> = vec![
            "100.64.0.42:49730".parse().unwrap(),
            "192.168.2.101:49730".parse().unwrap(),
            "192.168.2.16:49730".parse().unwrap(),
            "192.168.2.16:49730".parse().unwrap(),
            "127.0.0.1:49730".parse().unwrap(),
        ];
        assert_eq!(
            filter_pure_lan_candidate_ips(addrs, "192.168.2.101"),
            vec!["192.168.2.16".to_string()]
        );
    }

    #[test]
    fn edge_candidate_sorter_prioritizes_online_and_non_monitor_owner() {
        let mut candidates = vec![
            EdgeCandidateDto {
                id: "mac-studio".into(),
                display_name: "Mac Studio".into(),
                monitor_input: 17,
                hotkey_slot: 3,
                online: false,
            },
            EdgeCandidateDto {
                id: "win-desktop".into(),
                display_name: "Windows".into(),
                monitor_input: 15,
                hotkey_slot: 2,
                online: true,
            },
            EdgeCandidateDto {
                id: "macbook".into(),
                display_name: "MacBook".into(),
                monitor_input: 21,
                hotkey_slot: 1,
                online: true,
            },
        ];
        sort_edge_candidates_for_picker(&mut candidates, Some(21));
        assert_eq!(candidates[0].id, "win-desktop");
        assert_eq!(candidates[1].id, "macbook");
        assert_eq!(candidates[2].id, "mac-studio");
    }

    #[test]
    fn disconnected_cursor_host_recovers_to_provider_after_grace() {
        let mut cfg = AppConfig::default_home_setup("macbook", "test-token".to_string());
        cfg.input_provider_host_id = "macbook".to_string();
        let mut peer_online = HashMap::new();
        peer_online.insert("macbook".to_string(), true);
        peer_online.insert("win-desktop".to_string(), true);
        peer_online.insert("mac-studio".to_string(), false);
        let mut peer_edge_locked = HashMap::new();
        peer_edge_locked.insert("mac-studio".to_string(), true);

        let mut st = LiveState {
            cfg,
            deskflow_running: true,
            deskflow_connected: true,
            accessibility_trusted: true,
            display_sleeping: false,
            saved_ddc_brightness_before_sleep: None,
            external_display_online: true,
            local_probe: ConnectedMonitorProbe::default(),
            local_display_modes: Vec::new(),
            peer_detected_inputs: HashMap::new(),
            peer_detected_ports: HashMap::new(),
            peer_detected_modes: HashMap::new(),
            peer_display_modes: HashMap::new(),
            peer_monitor_model: None,
            active_cursor_host: Some("mac-studio".to_string()),
            deskflow_screen_host: Some("mac-studio".to_string()),
            observed_monitor_input: Some(21),
            current_brightness: Some(75),
            current_volume: Some(50),
            current_color_preset: Some(5),
            last_clipboard_png_sha: None,
            last_transfer_summary: None,
            edge_locked: false,
            peer_edge_locked,
            clamshell_closed: true,
            peer_clamshell_closed: HashMap::new(),
            applied_slide_peer_id: None,
            applied_mouse_adjustment: None,
            applied_confined_builtin: false,
            last_switch_activity_at: Instant::now() - Duration::from_secs(10),
            secure_input_active: false,
            last_error: None,
            peer_online,
            picker_visible: false,
            picker_bounds: None,
            edge_candidates: Vec::new(),
            hovered_candidate_id: None,
        };

        // Offline/untrusted target heals back to local input provider
        st.reconcile_disconnected_cursor_host(Duration::from_secs(5));
        assert_eq!(st.active_cursor_host.as_deref(), Some("macbook"));

        // Connected online target (win-desktop) remains focused
        st.active_cursor_host = Some("win-desktop".to_string());
        st.reconcile_disconnected_cursor_host(Duration::from_secs(5));
        assert_eq!(st.active_cursor_host.as_deref(), Some("win-desktop"));

        // Idle locked third peer (mac-studio) must never lock Deskflow or plain-slide between macbook & win-desktop
        assert!(!st.is_deskflow_session_locked());
        st.clamshell_closed = false;
        let win_input = st
            .cfg
            .hosts
            .iter()
            .find(|h| h.id == "win-desktop")
            .unwrap()
            .monitor_input;
        st.observed_monitor_input = Some(win_input);
        assert_eq!(st.effective_plain_slide_peer_id().as_deref(), Some("win-desktop"));
    }

    #[test]
    fn non_overwriting_path_reuses_identical_sha_and_suffixes_different_bytes() {
        let dir = std::env::temp_dir().join(format!("kvmddc-xfer-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let rel = Path::new("report.txt");
        let bytes_a = b"alpha";
        let sha_a = sha256_hex_bytes(bytes_a);
        let p1 = resolve_non_overwriting_path(&dir, rel, &sha_a);
        assert_eq!(p1, dir.join("report.txt"));
        std::fs::write(&p1, bytes_a).unwrap();

        let p_same = resolve_non_overwriting_path(&dir, rel, &sha_a);
        assert_eq!(p_same, dir.join("report.txt"));

        let sha_b = sha256_hex_bytes(b"beta");
        let p_diff = resolve_non_overwriting_path(&dir, rel, &sha_b);
        assert_eq!(p_diff, dir.join("report-2.txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_transfer_entries_walks_files_and_folders() {
        let dir = std::env::temp_dir().join(format!("kvmddc-walk-{}", uuid::Uuid::new_v4()));
        let sub = dir.join("Bundle").join("inner");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("note.md"), b"hello").unwrap();
        let solo = dir.join("pic.png");
        std::fs::write(&solo, b"png").unwrap();

        let entries = collect_transfer_entries(&[
            dir.join("Bundle").to_string_lossy().to_string(),
            solo.to_string_lossy().to_string(),
        ])
        .unwrap();
        assert_eq!(entries.len(), 2);
        let rels: Vec<String> = entries.into_iter().map(|e| e.rel_path).collect();
        assert!(rels.contains(&"Bundle/inner/note.md".to_string()));
        assert!(rels.contains(&"pic.png".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plain_slide_peer_locks_edges_when_clamshell_or_single_os_active() {
        let mut cfg = AppConfig::default_home_setup("macbook", "tok123".to_string());
        cfg.input_provider_host_id = "macbook".to_string();
        cfg.allow_plain_slide_to_internal = true;

        let mut peer_online = HashMap::new();
        peer_online.insert("macbook".to_string(), true);
        peer_online.insert("win-desktop".to_string(), true);

        let mut st = LiveState {
            cfg,
            deskflow_running: true,
            deskflow_connected: true,
            accessibility_trusted: true,
            display_sleeping: false,
            saved_ddc_brightness_before_sleep: None,
            external_display_online: true,
            local_probe: ConnectedMonitorProbe::default(),
            local_display_modes: Vec::new(),
            peer_detected_inputs: HashMap::new(),
            peer_detected_ports: HashMap::new(),
            peer_detected_modes: HashMap::new(),
            peer_display_modes: HashMap::new(),
            peer_monitor_model: None,
            active_cursor_host: Some("macbook".to_string()),
            deskflow_screen_host: Some("macbook".to_string()),
            observed_monitor_input: Some(21), // 27" monitor on macbook
            current_brightness: Some(80),
            current_volume: Some(75),
            current_color_preset: Some(5),
            last_clipboard_png_sha: None,
            last_transfer_summary: None,
            edge_locked: false,
            peer_edge_locked: HashMap::new(),
            clamshell_closed: true,
            peer_clamshell_closed: HashMap::new(),
            applied_slide_peer_id: None,
            applied_mouse_adjustment: None,
            applied_confined_builtin: false,
            last_switch_activity_at: Instant::now() - Duration::from_secs(10),
            secure_input_active: false,
            last_error: None,
            peer_online,
            picker_visible: false,
            picker_bounds: None,
            edge_candidates: Vec::new(),
            hovered_candidate_id: None,
        };

        // 1. Lid closed + 27" on MacBook -> locked (None)
        assert_eq!(st.effective_plain_slide_peer_id(), None);

        // 2. Lid closed + 27" on Windows -> still locked (None)
        st.observed_monitor_input = Some(17);
        assert_eq!(st.effective_plain_slide_peer_id(), None);

        // 3. Lid open + 27" on MacBook -> still locked (None, Windows has no visible screen)
        st.clamshell_closed = false;
        st.observed_monitor_input = Some(21);
        assert_eq!(st.effective_plain_slide_peer_id(), None);

        // 4. Lid open + 27" on Windows (online) -> unlocked to win-desktop!
        st.observed_monitor_input = Some(17);
        assert_eq!(
            st.effective_plain_slide_peer_id().as_deref(),
            Some("win-desktop")
        );

        // 5. Lid open + 27" on Windows (offline) -> locked (None)
        st.peer_online.insert("win-desktop".to_string(), false);
        assert_eq!(st.effective_plain_slide_peer_id(), None);
    }

    #[test]
    fn mouse_adjustment_scales_bidirectionally_between_macos_and_windows() {
        // 1. MacBook (2560 HiDPI logical) providing input to Windows (5120 physical) -> 2.00x
        let mut mac_srv = AppConfig::default_home_setup("macbook", "t".repeat(48));
        mac_srv.input_provider_host_id = "macbook".to_string();
        assert_eq!(
            compute_deskflow_mouse_adjustment(&mac_srv, Some(17)).as_deref(),
            Some("2.00")
        );

        // 2. Windows (5120 physical) providing input to MacBook (2560 HiDPI logical) -> 0.50x
        let mut win_srv = AppConfig::default_home_setup("win-desktop", "t".repeat(48));
        win_srv.input_provider_host_id = "win-desktop".to_string();
        assert_eq!(
            compute_deskflow_mouse_adjustment(&win_srv, Some(21)).as_deref(),
            Some("0.50")
        );

        // 3. Client node (not input provider) -> None
        assert_eq!(compute_deskflow_mouse_adjustment(&mac_srv_as_client(), Some(21)), None);

        // 4. MacBook (2560) providing input only to Mac Studio (2560 or 1920 HiDPI) -> None (1:1)
        let mut two_macs = AppConfig::default_home_setup("macbook", "t".repeat(48));
        two_macs.input_provider_host_id = "macbook".to_string();
        for h in &mut two_macs.hosts {
            if h.id == "win-desktop" {
                h.enabled = false;
            }
            if h.id == "mac-studio" {
                h.enabled = true;
            }
        }
        assert_eq!(compute_deskflow_mouse_adjustment(&two_macs, Some(15)), None);
        for h in &mut two_macs.hosts {
            if h.id == "mac-studio" {
                h.display_profile.preferred_width = 1920;
                h.display_profile.preferred_height = 1080;
            }
        }
        assert_eq!(compute_deskflow_mouse_adjustment(&two_macs, Some(15)), None);
    }

    #[test]
    fn mouse_adjustment_3host_prefers_same_platform_unless_windows_active() {
        let mut tri = AppConfig::default_home_setup("macbook", "t".repeat(48));
        for h in &mut tri.hosts {
            h.enabled = true;
            if h.id == "win-desktop" {
                h.monitor_input = 15;
            } else if h.id == "mac-studio" {
                h.monitor_input = 17;
            } else if h.id == "macbook" {
                h.monitor_input = 21;
            }
        }

        // MacBook server (2560): only scale to 2.00 when Windows (15) is on the 27" monitor.
        tri.local_host_id = "macbook".to_string();
        tri.input_provider_host_id = "macbook".to_string();
        assert_eq!(compute_deskflow_mouse_adjustment(&tri, Some(21)), None);
        assert_eq!(compute_deskflow_mouse_adjustment(&tri, Some(17)), None);
        assert_eq!(
            compute_deskflow_mouse_adjustment(&tri, Some(15)).as_deref(),
            Some("2.00")
        );
        assert_eq!(compute_deskflow_mouse_adjustment(&tri, None), None);

        // Mac Studio server (2560): 1:1 with MacBook (21) or itself (17), 2.00 when Windows (15) active.
        tri.local_host_id = "mac-studio".to_string();
        tri.input_provider_host_id = "mac-studio".to_string();
        assert_eq!(compute_deskflow_mouse_adjustment(&tri, Some(17)), None);
        assert_eq!(compute_deskflow_mouse_adjustment(&tri, Some(21)), None);
        assert_eq!(
            compute_deskflow_mouse_adjustment(&tri, Some(15)).as_deref(),
            Some("2.00")
        );

        // Windows server (5120): always scales down 0.50 toward 2560 Mac clients.
        tri.local_host_id = "win-desktop".to_string();
        tri.input_provider_host_id = "win-desktop".to_string();
        assert_eq!(
            compute_deskflow_mouse_adjustment(&tri, Some(21)).as_deref(),
            Some("0.50")
        );
        assert_eq!(
            compute_deskflow_mouse_adjustment(&tri, Some(15)).as_deref(),
            Some("0.50")
        );
    }

    fn mac_srv_as_client() -> AppConfig {
        let mut c = AppConfig::default_home_setup("macbook", "t".repeat(48));
        c.input_provider_host_id = "win-desktop".to_string();
        c
    }

    #[test]
    fn clamshell_escape_target_and_client_reconnect_cursor_guard() {
        let mut cfg = AppConfig::default_home_setup("macbook", "tok123".to_string());
        cfg.input_provider_host_id = "macbook".to_string();
        for h in &mut cfg.hosts {
            h.enabled = true;
        }
        let mut peer_online = HashMap::new();
        peer_online.insert("macbook".to_string(), true);
        peer_online.insert("mac-studio".to_string(), true);
        peer_online.insert("win-desktop".to_string(), true);

        let mut st = LiveState {
            cfg,
            deskflow_running: true,
            deskflow_connected: true,
            accessibility_trusted: true,
            display_sleeping: false,
            saved_ddc_brightness_before_sleep: None,
            external_display_online: true, // stays true over USB-C even if monitor input is on Studio
            local_probe: ConnectedMonitorProbe::default(),
            local_display_modes: Vec::new(),
            peer_detected_inputs: HashMap::new(),
            peer_detected_ports: HashMap::new(),
            peer_detected_modes: HashMap::new(),
            peer_display_modes: HashMap::new(),
            peer_monitor_model: None,
            active_cursor_host: Some("mac-studio".to_string()),
            deskflow_screen_host: Some("mac-studio".to_string()),
            observed_monitor_input: Some(15), // mac-studio default port 15
            current_brightness: Some(80),
            current_volume: Some(75),
            current_color_preset: Some(5),
            last_clipboard_png_sha: None,
            last_transfer_summary: None,
            edge_locked: false,
            peer_edge_locked: HashMap::new(),
            clamshell_closed: true,
            peer_clamshell_closed: HashMap::new(),
            applied_slide_peer_id: None,
            applied_mouse_adjustment: None,
            applied_confined_builtin: false,
            last_switch_activity_at: Instant::now() - Duration::from_secs(10),
            secure_input_active: false,
            last_error: None,
            peer_online,
            picker_visible: false,
            picker_bounds: None,
            edge_candidates: Vec::new(),
            hovered_candidate_id: None,
        };

        // Closed MacBook while 27" monitor shows mac-studio escapes cursor to mac-studio.
        assert_eq!(
            st.monitor_escape_target_for_closed_laptop("macbook"),
            Some(("mac-studio".to_string(), 3))
        );

        // Client during transient Deskflow disconnect keeps remote focus while provider is online.
        st.cfg.local_host_id = "win-desktop".to_string();
        st.deskflow_connected = false;
        assert!(!st.has_local_cursor_for_controls());

        // If cursor really is on win-desktop, or provider went offline, local controls activate.
        st.active_cursor_host = Some("win-desktop".to_string());
        assert!(st.has_local_cursor_for_controls());
        st.active_cursor_host = Some("mac-studio".to_string());
        st.peer_online.insert("macbook".to_string(), false);
        assert!(st.has_local_cursor_for_controls());
    }

    #[test]
    fn peer_connect_and_disconnect_never_hijack_visible_local_cursor() {
        let mut cfg = AppConfig::default_home_setup("macbook", "tok123".to_string());
        cfg.input_provider_host_id = "macbook".to_string();
        cfg.allow_plain_slide_to_internal = false;
        for h in &mut cfg.hosts {
            h.enabled = true;
        }
        let mut peer_online = HashMap::new();
        peer_online.insert("macbook".to_string(), true);
        peer_online.insert("mac-studio".to_string(), true);
        peer_online.insert("win-desktop".to_string(), true);

        let mut st = LiveState {
            cfg,
            deskflow_running: true,
            deskflow_connected: true,
            accessibility_trusted: true,
            display_sleeping: false,
            saved_ddc_brightness_before_sleep: None,
            external_display_online: true,
            local_probe: ConnectedMonitorProbe::default(),
            local_display_modes: Vec::new(),
            peer_detected_inputs: HashMap::new(),
            peer_detected_ports: HashMap::new(),
            peer_detected_modes: HashMap::new(),
            peer_display_modes: HashMap::new(),
            peer_monitor_model: None,
            active_cursor_host: Some("mac-studio".to_string()),
            deskflow_screen_host: Some("mac-studio".to_string()),
            observed_monitor_input: Some(21), // 27" monitor is on MacBook (USB-C)
            current_brightness: Some(80),
            current_volume: Some(75),
            current_color_preset: Some(5),
            last_clipboard_png_sha: None,
            last_transfer_summary: None,
            edge_locked: false,
            peer_edge_locked: HashMap::new(),
            clamshell_closed: true,
            peer_clamshell_closed: HashMap::new(),
            applied_slide_peer_id: None,
            applied_mouse_adjustment: None,
            applied_confined_builtin: false,
            last_switch_activity_at: Instant::now() - Duration::from_secs(10),
            secure_input_active: false,
            last_error: None,
            peer_online,
            picker_visible: false,
            picker_bounds: None,
            edge_candidates: Vec::new(),
            hovered_candidate_id: None,
        };

        // 1. Stale active_cursor_host="mac-studio" while 27" monitor shows MacBook (21):
        // Mac Studio connecting must NOT emit F20 to steal focus off MacBook, and resets to macbook.
        let refocus = st.apply_deskflow_log_event(DeskflowLogEvent::PeerClientConnected {
            host_id: "mac-studio".to_string(),
        });
        assert_eq!(refocus, None);
        assert_eq!(st.active_cursor_host.as_deref(), Some("macbook"));

        // 2. If active_cursor_host="mac-studio" and Mac Studio disconnects, focus resets cleanly
        // to local macbook while keeping deskflow_connected=true (win-desktop is still online).
        st.active_cursor_host = Some("mac-studio".to_string());
        let after_dc = st.apply_deskflow_log_event(DeskflowLogEvent::PeerClientDisconnected {
            host_id: "mac-studio".to_string(),
        });
        assert_eq!(after_dc, None);
        assert_eq!(st.active_cursor_host.as_deref(), Some("macbook"));
        assert!(st.deskflow_connected);

        // 3. When 27" monitor actually displays win-desktop (17), win-desktop reconnecting DOES refocus.
        st.observed_monitor_input = Some(17);
        st.active_cursor_host = Some("win-desktop".to_string());
        let unrelated = st.apply_deskflow_log_event(DeskflowLogEvent::PeerClientConnected {
            host_id: "mac-studio".to_string(),
        });
        assert_eq!(unrelated, None);
        let win_refocus = st.apply_deskflow_log_event(DeskflowLogEvent::PeerClientConnected {
            host_id: "win-desktop".to_string(),
        });
        assert!(matches!(win_refocus, Some((ref h, _)) if h == "win-desktop"));
    }

    fn mk_ext_device(
        key: &str,
        kind: kvmddc_platform::ExternalInputKind,
        product: &str,
    ) -> ExternalInputDevice {
        ExternalInputDevice {
            device_key: key.to_string(),
            kind,
            product: product.to_string(),
            transport: "USB".to_string(),
        }
    }

    #[test]
    fn input_hotplug_tracker_ignores_baseline_and_standing_windows_devices() {
        let mut tracker = InputHotplugTracker::new();
        let t0 = Instant::now();
        let razer = mk_ext_device(
            "win:mouse:hid#vid_1532&pid_006e",
            kvmddc_platform::ExternalInputKind::Mouse,
            "VID_1532 PID_006E",
        );
        let rx_24g = mk_ext_device(
            "win:keyboard:hid#vid_3554&pid_fa09",
            kvmddc_platform::ExternalInputKind::Keyboard,
            "VID_3554 PID_FA09",
        );

        assert_eq!(
            tracker.poll(t0, &[razer.clone(), rx_24g.clone()], true, true),
            None
        );
        assert_eq!(
            tracker.poll(
                t0 + Duration::from_secs(10),
                &[razer.clone(), rx_24g.clone()],
                true,
                true
            ),
            None
        );
    }

    #[test]
    fn input_hotplug_tracker_debounces_arrivals_and_suppresses_glitches_and_self_bounces() {
        let mut tracker = InputHotplugTracker::new();
        let t0 = Instant::now();
        let mouse = mk_ext_device(
            "mac:mouse:usb:17ef:6225:1110000",
            kvmddc_platform::ExternalInputKind::Mouse,
            "tp T50Pro",
        );
        let kb = mk_ext_device(
            "mac:keyboard:usb:258a:010c:1120000",
            kvmddc_platform::ExternalInputKind::Keyboard,
            "Gaming Keyboard",
        );
        let glitch = mk_ext_device(
            "mac:mouse:usb:1234:5678:1130000",
            kvmddc_platform::ExternalInputKind::Mouse,
            "Glitch Mouse",
        );

        // Baseline starts empty (hub not yet plugged into this Mac).
        assert_eq!(tracker.poll(t0, &[], true, true), None);

        // Transient glitch vanishes before 700ms settle window -> never claims.
        let t1 = t0 + Duration::from_secs(1);
        assert_eq!(tracker.poll(t1, &[glitch], true, true), None);
        assert_eq!(
            tracker.poll(t1 + Duration::from_millis(300), &[], true, true),
            None
        );
        assert_eq!(
            tracker.poll(t1 + Duration::from_millis(900), &[], true, true),
            None
        );

        // Real hub arrives: mouse enumerates at t2, keyboard follows at t2+200ms.
        let t2 = t0 + Duration::from_secs(3);
        assert_eq!(tracker.poll(t2, &[mouse.clone()], true, true), None);
        assert_eq!(
            tracker.poll(
                t2 + Duration::from_millis(200),
                &[mouse.clone(), kb.clone()],
                true,
                true
            ),
            None
        );

        // At t2+750ms (>=700ms settle), both devices claim together once; next poll is quiet.
        let claimed = tracker
            .poll(
                t2 + Duration::from_millis(750),
                &[mouse.clone(), kb.clone()],
                true,
                true,
            )
            .expect("hub should claim after settle window");
        assert_eq!(claimed.len(), 2);
        assert_eq!(
            tracker.poll(
                t2 + Duration::from_millis(1200),
                &[mouse.clone(), kb.clone()],
                true,
                true
            ),
            None
        );

        // Brief 1s USB drop + replug (<2.5s self-bounce grace) must NOT re-trigger.
        let t3 = t0 + Duration::from_secs(10);
        assert_eq!(tracker.poll(t3, &[], true, true), None);
        assert_eq!(
            tracker.poll(
                t3 + Duration::from_secs(1),
                &[mouse.clone(), kb.clone()],
                true,
                true
            ),
            None
        );
        assert_eq!(
            tracker.poll(t3 + Duration::from_secs(2), &[mouse, kb], true, true),
            None
        );
    }

    #[test]
    fn input_hotplug_tracker_defers_until_accessibility_trusted_and_drops_if_unplugged() {
        let mut tracker = InputHotplugTracker::new();
        let t0 = Instant::now();
        let mouse = mk_ext_device(
            "mac:mouse:usb:17ef:6225:2110000",
            kvmddc_platform::ExternalInputKind::Mouse,
            "tp T50Pro",
        );

        assert_eq!(tracker.poll(t0, &[], true, false), None);
        assert_eq!(
            tracker.poll(t0 + Duration::from_secs(1), &[mouse.clone()], true, false),
            None
        );
        // Settles while macOS Accessibility TCC is still untrusted -> stays deferred.
        assert_eq!(
            tracker.poll(t0 + Duration::from_secs(2), &[mouse.clone()], true, false),
            None
        );
        // Once user toggles TCC on while device is still attached -> claims immediately.
        let claimed = tracker
            .poll(t0 + Duration::from_millis(2400), &[mouse.clone()], true, true)
            .expect("deferred device should claim once Accessibility becomes trusted");
        assert_eq!(claimed, vec![mouse.clone()]);

        // Wait >2.5s unplugged, replug while untrusted, unplug before granting -> never claims.
        assert_eq!(tracker.poll(t0 + Duration::from_secs(5), &[], true, false), None);
        assert_eq!(
            tracker.poll(t0 + Duration::from_secs(9), &[mouse.clone()], true, false),
            None
        );
        assert_eq!(
            tracker.poll(t0 + Duration::from_secs(10), &[mouse], true, false),
            None
        );
        assert_eq!(
            tracker.poll(t0 + Duration::from_secs(11), &[], true, false),
            None
        );
        assert_eq!(
            tracker.poll(t0 + Duration::from_secs(12), &[], true, true),
            None
        );
    }

    #[test]
    fn peer_shape_change_and_anonymous_tls_disconnect_preserve_active_monitor_focus() {
        let mut cfg = AppConfig::default_home_setup("macbook", "test-token".to_string());
        cfg.input_provider_host_id = "macbook".to_string();
        cfg.allow_plain_slide_to_internal = true;

        let win_host = cfg
            .hosts
            .iter()
            .find(|h| h.id == "win-desktop")
            .cloned()
            .unwrap();

        let mut peer_online = HashMap::new();
        peer_online.insert("macbook".to_string(), true);
        peer_online.insert("win-desktop".to_string(), true);
        peer_online.insert("mac-studio".to_string(), false);

        let mut st = LiveState {
            cfg,
            deskflow_running: true,
            deskflow_connected: true,
            accessibility_trusted: true,
            display_sleeping: false,
            saved_ddc_brightness_before_sleep: None,
            external_display_online: true,
            local_probe: ConnectedMonitorProbe::default(),
            local_display_modes: Vec::new(),
            peer_detected_inputs: HashMap::new(),
            peer_detected_ports: HashMap::new(),
            peer_detected_modes: HashMap::new(),
            peer_display_modes: HashMap::new(),
            peer_monitor_model: None,
            active_cursor_host: Some("macbook".to_string()),
            deskflow_screen_host: Some("macbook".to_string()),
            observed_monitor_input: Some(win_host.monitor_input),
            current_brightness: Some(75),
            current_volume: Some(50),
            current_color_preset: Some(5),
            last_clipboard_png_sha: None,
            last_transfer_summary: None,
            edge_locked: false,
            peer_edge_locked: HashMap::new(),
            clamshell_closed: false,
            peer_clamshell_closed: HashMap::new(),
            applied_slide_peer_id: Some("win-desktop".to_string()),
            applied_mouse_adjustment: None,
            applied_confined_builtin: true,
            last_switch_activity_at: Instant::now() - Duration::from_secs(10),
            secure_input_active: false,
            last_error: None,
            peer_online,
            picker_visible: false,
            picker_bounds: None,
            edge_candidates: Vec::new(),
            hovered_candidate_id: None,
        };

        // Reconnecting win-desktop while it owns the 27" monitor immediately snaps focus to it.
        assert_eq!(
            st.apply_deskflow_log_event(DeskflowLogEvent::PeerClientConnected {
                host_id: "win-desktop".to_string(),
            }),
            Some(("win-desktop".to_string(), win_host.hotkey_slot))
        );
        assert_eq!(st.active_cursor_host.as_deref(), Some("win-desktop"));

        // Resolution change / DP wake emits PeerShapeChanged + involuntary reset to macbook:
        // Kvmddc must refuse stranding on macbook and immediately refocus win-desktop!
        assert_eq!(
            st.apply_deskflow_log_event(DeskflowLogEvent::PeerShapeChanged {
                host_id: "win-desktop".to_string(),
            }),
            Some(("win-desktop".to_string(), win_host.hotkey_slot))
        );
        assert_eq!(
            st.apply_deskflow_log_event(DeskflowLogEvent::Switched {
                from: "win-desktop".to_string(),
                to: "macbook".to_string(),
            }),
            Some(("win-desktop".to_string(), win_host.hotkey_slot))
        );
        assert_eq!(st.active_cursor_host.as_deref(), Some("win-desktop"));

        // Anonymous `tls connection closed` from a 3rd peer while win-desktop is online must NOT
        // drop `deskflow_connected` or reset `active_cursor_host` back to `macbook`.
        assert_eq!(
            st.apply_deskflow_log_event(DeskflowLogEvent::ClientDisconnected),
            None
        );
        assert!(st.deskflow_connected);
        assert_eq!(st.active_cursor_host.as_deref(), Some("win-desktop"));

        // Once shape-change window expires (> 2.2s), a real user slide to open MacBook is honored.
        *LAST_PEER_SHAPE_CHANGE.lock().unwrap() =
            Some(("win-desktop".to_string(), Instant::now() - Duration::from_secs(5)));
        st.last_switch_activity_at = Instant::now() - Duration::from_secs(5);
        assert_eq!(
            st.apply_deskflow_log_event(DeskflowLogEvent::Switched {
                from: "win-desktop".to_string(),
                to: "macbook".to_string(),
            }),
            None
        );
        assert_eq!(st.active_cursor_host.as_deref(), Some("macbook"));
    }

    #[test]
    fn user_chosen_desktop_resolution_adopts_hd_qhd_uhd_and_rejects_windisc_fallback() {
        assert!(!is_user_chosen_desktop_resolution(640, 480));
        assert!(!is_user_chosen_desktop_resolution(1024, 768));
        assert!(is_user_chosen_desktop_resolution(1280, 720));
        assert!(is_user_chosen_desktop_resolution(1920, 1080));
        assert!(is_user_chosen_desktop_resolution(2560, 1440));
        assert!(is_user_chosen_desktop_resolution(3840, 2160));
        assert!(is_user_chosen_desktop_resolution(5120, 2880));
    }

    #[test]
    fn closed_macbook_accepts_voluntary_switch_from_studio_without_bounce_back() {
        let mut cfg = AppConfig::default_home_setup("macbook", "tok123".to_string());
        cfg.input_provider_host_id = "macbook".to_string();
        for h in &mut cfg.hosts {
            h.enabled = true;
            if h.id == "mac-studio" {
                h.monitor_input = 17;
            } else if h.id == "win-desktop" {
                h.monitor_input = 15;
            }
        }
        let mut peer_online = HashMap::new();
        peer_online.insert("macbook".to_string(), true);
        peer_online.insert("mac-studio".to_string(), true);
        peer_online.insert("win-desktop".to_string(), true);

        let mut st = LiveState {
            cfg,
            deskflow_running: true,
            deskflow_connected: true,
            accessibility_trusted: true,
            display_sleeping: false,
            saved_ddc_brightness_before_sleep: None,
            external_display_online: true,
            local_probe: ConnectedMonitorProbe::default(),
            local_display_modes: Vec::new(),
            peer_detected_inputs: HashMap::new(),
            peer_detected_ports: HashMap::new(),
            peer_detected_modes: HashMap::new(),
            peer_display_modes: HashMap::new(),
            peer_monitor_model: None,
            active_cursor_host: Some("mac-studio".to_string()),
            deskflow_screen_host: Some("mac-studio".to_string()),
            observed_monitor_input: Some(17),
            current_brightness: Some(80),
            current_volume: Some(75),
            current_color_preset: Some(5),
            last_clipboard_png_sha: None,
            last_transfer_summary: None,
            edge_locked: false,
            peer_edge_locked: HashMap::new(),
            clamshell_closed: true, // MacBook lid closed (27" monitor is its only screen!)
            peer_clamshell_closed: HashMap::new(),
            applied_slide_peer_id: None,
            applied_mouse_adjustment: None,
            applied_confined_builtin: false,
            last_switch_activity_at: Instant::now(),
            secure_input_active: false,
            last_error: None,
            peer_online,
            picker_visible: false,
            picker_bounds: None,
            edge_candidates: Vec::new(),
            hovered_candidate_id: None,
        };

        // Even if /v1/focus arrives while observed_monitor_input is momentarily still 17 (mac-studio),
        // setting active_cursor_host = Some("macbook") marks the switch voluntary: Deskflow's
        // `Switched { to: "macbook" }` and `EnteringScreen` MUST NOT bounce back to mac-studio!
        st.active_cursor_host = Some("macbook".to_string());
        st.last_switch_activity_at = Instant::now();
        assert_eq!(st.involuntary_local_reset_escape_target(), None);
        assert_eq!(
            st.apply_deskflow_log_event(DeskflowLogEvent::Switched {
                from: "mac-studio".to_string(),
                to: "macbook".to_string(),
            }),
            None
        );
        assert_eq!(
            st.apply_deskflow_log_event(DeskflowLogEvent::EnteringScreen),
            None
        );
        assert_eq!(st.active_cursor_host.as_deref(), Some("macbook"));
        assert_eq!(st.deskflow_screen_host.as_deref(), Some("macbook"));

        // Once 27" monitor input is 21 (MacBook), closed-laptop escape returns None because
        // MacBook IS the active screen on the 27" external monitor.
        st.observed_monitor_input = Some(21);
        assert_eq!(st.monitor_escape_target_for_closed_laptop("macbook"), None);
    }
}
