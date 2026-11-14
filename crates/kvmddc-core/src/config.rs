use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostPlatform {
    Macos,
    Windows,
}

impl HostPlatform {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Macos
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DisplayProfile {
    pub preferred_width: u32,
    pub preferred_height: u32,
    pub preferred_hz: u32,
    pub has_internal_screen: bool,
    /// Percentage (1..=100) of total desktop height occupied by internal screen on the right side
    pub internal_height_percent: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostNode {
    pub id: String,
    pub display_name: String,
    pub platform: HostPlatform,
    pub lan_ip: String,
    pub mdns_fallback: Option<String>,
    pub monitor_input: u8,
    pub hotkey_slot: u8,
    pub enabled: bool,
    pub mac_address: Option<String>,
    pub tls_fingerprint: Option<String>,
    #[serde(default)]
    pub manual_port_override: bool,
    #[serde(default)]
    pub preferred_brightness: Option<u8>,
    #[serde(default)]
    pub preferred_color_preset: Option<u8>,
    pub display_profile: DisplayProfile,
}

fn default_true() -> bool {
    true
}

fn default_volume() -> u8 {
    75
}

fn default_color_preset() -> u8 {
    5 // MCCS 0x14 = 5 (6500K standard on Philips 27E2N6900RW)
}

pub fn color_preset_label(code: u8) -> &'static str {
    match code {
        4 => "5000K 暖光",
        5 => "6500K 标准",
        6 => "7500K 清冷",
        8 => "9300K 冷白",
        2 => "原生色温",
        _ => "6500K 标准",
    }
}

pub fn next_color_preset(current: u8) -> u8 {
    match current {
        5 => 4,
        4 => 6,
        6 => 8,
        8 => 5,
        _ => 5,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedClusterSettings {
    pub rev: u64,
    pub input_provider_host_id: Option<String>,
    pub primary_ddc_controller_id: Option<String>,
    pub deskflow_enabled: Option<bool>,
    pub ctrl_edge_switch_enabled: Option<bool>,
    pub allow_plain_slide_to_internal: Option<bool>,
    pub unify_cross_os_modifiers: Option<bool>,
    pub auto_switch_input_provider_on_hotplug: Option<bool>,
    pub internal_height_percent: Option<u8>,
}

impl SharedClusterSettings {
    pub fn has_any_field(&self) -> bool {
        self.input_provider_host_id.is_some()
            || self.primary_ddc_controller_id.is_some()
            || self.deskflow_enabled.is_some()
            || self.ctrl_edge_switch_enabled.is_some()
            || self.allow_plain_slide_to_internal.is_some()
            || self.unify_cross_os_modifiers.is_some()
            || self.auto_switch_input_provider_on_hotplug.is_some()
            || self.internal_height_percent.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedSyncOutcome {
    pub changed: bool,
    pub deskflow_reload_needed: bool,
    pub push_back_to_source: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: u32,
    #[serde(default)]
    pub shared_settings_rev: u64,
    pub local_host_id: String,
    /// Host ID acting as Deskflow keyboard/mouse provider (Server)
    pub input_provider_host_id: String,
    /// Primary Host ID acting as DDC/CI controller (must be Mac or DDC-capable host)
    pub primary_ddc_controller_id: String,
    pub control_port: u16,
    pub deskflow_port: u16,
    pub shared_token: String,
    /// Physical monitor handshake settle window in milliseconds (default 5000ms)
    pub settle_ms: u64,
    /// Ctrl + screen edge dwell milliseconds before switching / showing picker (default 180ms)
    pub ctrl_edge_dwell_ms: u64,
    /// Deskflow server switchDelay in ms for non-Ctrl internal-screen sliding (default 120ms)
    pub deskflow_switch_delay_ms: u32,
    /// Master switch for Deskflow cross-host keyboard/mouse sharing daemon
    #[serde(default = "default_true")]
    pub deskflow_enabled: bool,
    /// Enable holding Ctrl + pushing screen edge to switch monitor & input
    #[serde(default = "default_true")]
    pub ctrl_edge_switch_enabled: bool,
    /// Allow sliding across right edge without Ctrl onto MacBook 14" screen without switching 27" monitor
    pub allow_plain_slide_to_internal: bool,
    /// Automatically align Command/Option/Control <-> Ctrl/Alt/Win when crossing macOS <-> Windows
    #[serde(default = "default_true")]
    pub unify_cross_os_modifiers: bool,
    /// Automatically switch Deskflow input provider when external USB/Bluetooth mouse or keyboard is hotplugged
    #[serde(default = "default_true")]
    pub auto_switch_input_provider_on_hotplug: bool,
    pub preferred_brightness: u8,
    #[serde(default = "default_volume")]
    pub preferred_volume: u8,
    #[serde(default)]
    pub audio_muted: bool,
    #[serde(default = "default_color_preset")]
    pub preferred_color_preset: u8,
    pub deskflow_binary_override: Option<String>,
    pub hosts: Vec<HostNode>,
}

impl AppConfig {
    pub fn default_home_setup(local_host_id: &str, shared_token: String) -> Self {
        Self {
            version: 1,
            shared_settings_rev: 0,
            local_host_id: local_host_id.to_string(),
            input_provider_host_id: "win-desktop".to_string(),
            primary_ddc_controller_id: "macbook".to_string(),
            control_port: 49730,
            deskflow_port: 24800,
            shared_token,
            settle_ms: 5000,
            ctrl_edge_dwell_ms: 180,
            deskflow_switch_delay_ms: 120,
            deskflow_enabled: true,
            ctrl_edge_switch_enabled: true,
            allow_plain_slide_to_internal: true,
            unify_cross_os_modifiers: true,
            auto_switch_input_provider_on_hotplug: true,
            preferred_brightness: 85,
            preferred_volume: 75,
            audio_muted: false,
            preferred_color_preset: 5,
            deskflow_binary_override: None,
            hosts: vec![
                HostNode {
                    id: "macbook".to_string(),
                    display_name: "MacBook Pro 14".to_string(),
                    platform: HostPlatform::Macos,
                    lan_ip: "192.168.1.10".to_string(),
                    mdns_fallback: Some("MacBook-Pro.local".to_string()),
                    monitor_input: 21,
                    hotkey_slot: 1,
                    enabled: true,
                    mac_address: None,
                    tls_fingerprint: None,
                    manual_port_override: false,
                    preferred_brightness: Some(85),
                    preferred_color_preset: Some(5),
                    display_profile: DisplayProfile {
                        preferred_width: 2560,
                        preferred_height: 1440,
                        preferred_hz: 75,
                        has_internal_screen: true,
                        internal_height_percent: 81,
                    },
                },
                HostNode {
                    id: "win-desktop".to_string(),
                    display_name: "Windows 台式机".to_string(),
                    platform: HostPlatform::Windows,
                    lan_ip: "192.168.1.20".to_string(),
                    mdns_fallback: None,
                    monitor_input: 17,
                    hotkey_slot: 2,
                    enabled: true,
                    mac_address: None,
                    tls_fingerprint: None,
                    manual_port_override: false,
                    preferred_brightness: Some(88),
                    preferred_color_preset: Some(5),
                    display_profile: DisplayProfile {
                        preferred_width: 5120,
                        preferred_height: 2880,
                        preferred_hz: 75,
                        has_internal_screen: false,
                        internal_height_percent: 100,
                    },
                },
                HostNode {
                    id: "mac-studio".to_string(),
                    display_name: "Mac Studio M5 Max".to_string(),
                    platform: HostPlatform::Macos,
                    lan_ip: "192.168.1.30".to_string(),
                    mdns_fallback: Some("Mac-Studio.local".to_string()),
                    monitor_input: 15,
                    hotkey_slot: 3,
                    enabled: false,
                    mac_address: None,
                    tls_fingerprint: None,
                    manual_port_override: false,
                    preferred_brightness: Some(85),
                    preferred_color_preset: Some(5),
                    display_profile: DisplayProfile {
                        preferred_width: 2560,
                        preferred_height: 1440,
                        preferred_hz: 75,
                        has_internal_screen: false,
                        internal_height_percent: 100,
                    },
                },
            ],
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(!self.shared_token.trim().is_empty(), "共享令牌不能为空");
        ensure!(self.control_port >= 1024, "控制端口必须 >= 1024");
        ensure!(self.deskflow_port >= 1024, "Deskflow 端口必须 >= 1024");
        ensure!(
            self.control_port != self.deskflow_port,
            "控制端口不能与 Deskflow 端口相同"
        );
        ensure!(
            (1000..=15000).contains(&self.settle_ms),
            "显示器重握手等待窗口需在 1000..=15000ms 之间"
        );
        ensure!(
            self.preferred_brightness <= 100,
            "目标亮度必须在 0..=100 之间"
        );
        ensure!(
            self.preferred_volume <= 100,
            "目标音量必须在 0..=100 之间"
        );
        ensure!(!self.hosts.is_empty(), "主机列表不能为空");

        for h in &self.hosts {
            validate_pure_lan_ip(&h.lan_ip)
                .with_context(|| format!("主机 {} 的 IP 不符合纯局域网要求", h.id))?;
            ensure!(h.monitor_input >= 1, "输入源编码必须为 1..=255");
            if let Some(hb) = h.preferred_brightness {
                ensure!(hb <= 100, "主机预设亮度必须在 0..=100 之间");
            }
        }

        ensure!(
            self.hosts.iter().any(|h| h.id == self.local_host_id),
            "local_host_id ({}) 不在主机列表中",
            self.local_host_id
        );
        ensure!(
            self.hosts.iter().any(|h| h.id == self.input_provider_host_id && h.enabled),
            "键鼠提供端 ({}) 必须存在于已启用主机中",
            self.input_provider_host_id
        );
        ensure!(
            self.hosts
                .iter()
                .any(|h| h.id == self.primary_ddc_controller_id && h.enabled),
            "DDC 控制端 ({}) 必须存在于已启用主机中",
            self.primary_ddc_controller_id
        );
        Ok(())
    }

    pub fn local_host(&self) -> Result<&HostNode> {
        self.hosts
            .iter()
            .find(|h| h.id == self.local_host_id)
            .context("找不到本机节点配置")
    }

    pub fn enabled_hosts(&self) -> Vec<&HostNode> {
        self.hosts.iter().filter(|h| h.enabled).collect()
    }

    pub fn is_input_provider(&self) -> bool {
        self.local_host_id == self.input_provider_host_id
    }

    pub fn is_ddc_controller(&self) -> bool {
        self.local_host_id == self.primary_ddc_controller_id
    }

    pub fn load_or_init(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let path = Self::config_path(data_dir);
        if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("读取配置失败: {}", path.display()))?;
            let cfg: Self = serde_json::from_str(&raw)
                .with_context(|| format!("解析配置失败: {}", path.display()))?;
            cfg.validate()?;
            return Ok(cfg);
        }

        let default_host_id = if cfg!(target_os = "windows") {
            "win-desktop"
        } else {
            "macbook"
        };
        let token = uuid::Uuid::new_v4().simple().to_string();
        let cfg = Self::default_home_setup(default_host_id, token);
        cfg.save(data_dir)?;
        Ok(cfg)
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        self.validate()?;
        std::fs::create_dir_all(data_dir)?;
        let path = Self::config_path(data_dir);
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(self)?;
        std::fs::write(&tmp, body)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn config_path(data_dir: &Path) -> PathBuf {
        data_dir.join("config.local.json")
    }

    /// Reconciles a live hardware-detected monitor input code (21=USB-C, 17=HDMI, 15=DP) for `host_id`.
    /// Automatically vacates any stale colliding host to the remaining free monitor port so swapping
    /// HDMI / DP cables between Windows and Mac Studio works zero-touch without port collision.
    pub fn reconcile_detected_port(
        &mut self,
        host_id: &str,
        detected_input: u8,
        auto_enable_peer: bool,
    ) -> bool {
        if detected_input == 0 {
            return false;
        }
        let Some(target_idx) = self.hosts.iter().position(|h| h.id == host_id) else {
            return false;
        };
        if self.hosts[target_idx].manual_port_override {
            return false;
        }

        let prev_input = self.hosts[target_idx].monitor_input;
        let mut changed = false;

        if auto_enable_peer && !self.hosts[target_idx].enabled {
            self.hosts[target_idx].enabled = true;
            changed = true;
        }

        if prev_input != detected_input {
            self.hosts[target_idx].monitor_input = detected_input;
            changed = true;
        }

        // Resolve any collision where another non-overridden host still holds `detected_input`
        let known_ports = [21u8, 17u8, 15u8];
        for idx in 0..self.hosts.len() {
            if idx == target_idx {
                continue;
            }
            if self.hosts[idx].monitor_input == detected_input {
                // Prefer swapping to `prev_input` if unused, else pick first unused port from [21, 17, 15]
                let prev_taken = self
                    .hosts
                    .iter()
                    .enumerate()
                    .any(|(j, h)| j != idx && h.monitor_input == prev_input);
                let replacement = if prev_input != 0 && prev_input != detected_input && !prev_taken {
                    prev_input
                } else {
                    known_ports
                        .iter()
                        .copied()
                        .find(|&cand| {
                            !self
                                .hosts
                                .iter()
                                .enumerate()
                                .any(|(j, h)| j != idx && h.monitor_input == cand)
                        })
                        .unwrap_or(prev_input)
                };
                if self.hosts[idx].monitor_input != replacement && replacement != 0 {
                    self.hosts[idx].monitor_input = replacement;
                    changed = true;
                }
            }
        }

        changed
    }

    /// Explicitly sets a host's monitor input port (`None` clears manual override and restores auto-detection).
    pub fn set_host_port_override(&mut self, host_id: &str, manual_input: Option<u8>) -> bool {
        let Some(idx) = self.hosts.iter().position(|h| h.id == host_id) else {
            return false;
        };
        match manual_input {
            None => {
                if self.hosts[idx].manual_port_override {
                    self.hosts[idx].manual_port_override = false;
                    true
                } else {
                    false
                }
            }
            Some(code) if code >= 1 => {
                let prev = self.hosts[idx].monitor_input;
                self.hosts[idx].manual_port_override = false;
                let mut changed = self.reconcile_detected_port(host_id, code, false);
                self.hosts[idx].manual_port_override = true;
                if prev == code {
                    changed = true;
                }
                changed
            }
            _ => false,
        }
    }

    pub fn internal_height_percent(&self) -> u8 {
        self.hosts
            .iter()
            .find(|h| h.display_profile.has_internal_screen)
            .map(|h| h.display_profile.internal_height_percent.clamp(60, 100))
            .unwrap_or(81)
    }

    pub fn set_internal_height_percent(&mut self, percent: u8) -> bool {
        let clamped = percent.clamp(60, 100);
        let mut changed = false;
        for h in &mut self.hosts {
            if h.display_profile.has_internal_screen
                && h.display_profile.internal_height_percent != clamped
            {
                h.display_profile.internal_height_percent = clamped;
                changed = true;
            }
        }
        changed
    }

    pub fn bump_shared_settings_rev(&mut self) {
        let now_ms = chrono::Utc::now().timestamp_millis().max(1) as u64;
        self.shared_settings_rev = now_ms.max(self.shared_settings_rev.saturating_add(1));
    }

    pub fn shared_settings_snapshot(&self) -> SharedClusterSettings {
        SharedClusterSettings {
            rev: self.shared_settings_rev,
            input_provider_host_id: Some(self.input_provider_host_id.clone()),
            primary_ddc_controller_id: Some(self.primary_ddc_controller_id.clone()),
            deskflow_enabled: Some(self.deskflow_enabled),
            ctrl_edge_switch_enabled: Some(self.ctrl_edge_switch_enabled),
            allow_plain_slide_to_internal: Some(self.allow_plain_slide_to_internal),
            unify_cross_os_modifiers: Some(self.unify_cross_os_modifiers),
            auto_switch_input_provider_on_hotplug: Some(self.auto_switch_input_provider_on_hotplug),
            internal_height_percent: Some(self.internal_height_percent()),
        }
    }

    fn differs_from_shared(&self, incoming: &SharedClusterSettings) -> bool {
        if let Some(ref p) = incoming.input_provider_host_id {
            if &self.input_provider_host_id != p && self.hosts.iter().any(|h| &h.id == p) {
                return true;
            }
        }
        if let Some(ref c) = incoming.primary_ddc_controller_id {
            if &self.primary_ddc_controller_id != c && self.hosts.iter().any(|h| &h.id == c) {
                return true;
            }
        }
        if incoming
            .deskflow_enabled
            .is_some_and(|v| v != self.deskflow_enabled)
        {
            return true;
        }
        if incoming
            .ctrl_edge_switch_enabled
            .is_some_and(|v| v != self.ctrl_edge_switch_enabled)
        {
            return true;
        }
        if incoming
            .allow_plain_slide_to_internal
            .is_some_and(|v| v != self.allow_plain_slide_to_internal)
        {
            return true;
        }
        if incoming
            .unify_cross_os_modifiers
            .is_some_and(|v| v != self.unify_cross_os_modifiers)
        {
            return true;
        }
        if incoming
            .auto_switch_input_provider_on_hotplug
            .is_some_and(|v| v != self.auto_switch_input_provider_on_hotplug)
        {
            return true;
        }
        if incoming
            .internal_height_percent
            .is_some_and(|v| v.clamp(60, 100) != self.internal_height_percent())
        {
            return true;
        }
        false
    }

    /// Reconciles cluster-wide shared settings using revision ordering (`rev`) plus a loop-free
    /// deterministic `host_id` tie-breaker so split-brain hosts converge cleanly without stale overwrites.
    pub fn reconcile_shared_settings(
        &mut self,
        incoming: &SharedClusterSettings,
        source_host_id: &str,
    ) -> SharedSyncOutcome {
        if !incoming.has_any_field() {
            return SharedSyncOutcome {
                changed: false,
                deskflow_reload_needed: false,
                push_back_to_source: false,
            };
        }

        if incoming.rev < self.shared_settings_rev {
            return SharedSyncOutcome {
                changed: false,
                deskflow_reload_needed: false,
                push_back_to_source: true,
            };
        }

        if incoming.rev == self.shared_settings_rev {
            if !self.differs_from_shared(incoming) {
                return SharedSyncOutcome {
                    changed: false,
                    deskflow_reload_needed: false,
                    push_back_to_source: false,
                };
            }
            if source_host_id >= self.local_host_id.as_str() {
                // Deterministic tie-breaker won by `self`: promote to a strictly newer revision and push back.
                self.bump_shared_settings_rev();
                return SharedSyncOutcome {
                    changed: true,
                    deskflow_reload_needed: false,
                    push_back_to_source: true,
                };
            }
        }

        let mut changed = false;
        let mut reload = false;

        if let Some(ref p) = incoming.input_provider_host_id {
            if let Some(h) = self.hosts.iter_mut().find(|h| &h.id == p) {
                if !h.enabled {
                    h.enabled = true;
                    changed = true;
                    reload = true;
                }
                if &self.input_provider_host_id != p {
                    self.input_provider_host_id = p.clone();
                    changed = true;
                    reload = true;
                }
            }
        }
        if let Some(ref c) = incoming.primary_ddc_controller_id {
            if let Some(h) = self.hosts.iter_mut().find(|h| &h.id == c) {
                if !h.enabled {
                    h.enabled = true;
                    changed = true;
                }
                if &self.primary_ddc_controller_id != c {
                    self.primary_ddc_controller_id = c.clone();
                    changed = true;
                }
            }
        }
        if let Some(v) = incoming.deskflow_enabled {
            if self.deskflow_enabled != v {
                self.deskflow_enabled = v;
                changed = true;
                reload = true;
            }
        }
        if let Some(v) = incoming.ctrl_edge_switch_enabled {
            if self.ctrl_edge_switch_enabled != v {
                self.ctrl_edge_switch_enabled = v;
                changed = true;
            }
        }
        if let Some(v) = incoming.allow_plain_slide_to_internal {
            if self.allow_plain_slide_to_internal != v {
                self.allow_plain_slide_to_internal = v;
                changed = true;
                reload = true;
            }
        }
        if let Some(v) = incoming.unify_cross_os_modifiers {
            if self.unify_cross_os_modifiers != v {
                self.unify_cross_os_modifiers = v;
                changed = true;
                reload = true;
            }
        }
        if let Some(v) = incoming.auto_switch_input_provider_on_hotplug {
            if self.auto_switch_input_provider_on_hotplug != v {
                self.auto_switch_input_provider_on_hotplug = v;
                changed = true;
            }
        }
        if let Some(pct) = incoming.internal_height_percent {
            if self.set_internal_height_percent(pct) {
                changed = true;
                reload = true;
            }
        }
        if self.shared_settings_rev != incoming.rev {
            self.shared_settings_rev = incoming.rev;
            changed = true;
        }

        SharedSyncOutcome {
            changed,
            deskflow_reload_needed: reload,
            push_back_to_source: false,
        }
    }
}

/// Enforces J1 hard rule: strictly pure LAN IPv4 (or 127.0.0.1 for local tests), rejecting Tailscale 100.64.0.0/10.
pub fn validate_pure_lan_ip(ip_str: &str) -> Result<Ipv4Addr> {
    let ip: IpAddr = ip_str
        .parse()
        .with_context(|| format!("无效 IP 地址: {ip_str}"))?;
    let IpAddr::V4(v4) = ip else {
        bail!("仅支持局域网 IPv4 地址: {ip_str}");
    };
    let octets = v4.octets();
    if octets[0] == 100 && (64..=127).contains(&octets[1]) {
        bail!("检测到 Tailscale 网段地址 ({ip_str})，违反纯局域网硬规则");
    }
    if v4.is_unspecified() {
        bail!("禁止绑定 0.0.0.0，必须显式填写局域网网卡 IP");
    }
    let is_rfc1918 = octets[0] == 10
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 168);
    if !is_rfc1918 && !v4.is_loopback() {
        bail!("地址 {ip_str} 不属于私有局域网段");
    }
    Ok(v4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_tailscale_and_unspecified_ips() {
        assert!(validate_pure_lan_ip("100.64.0.42").is_err());
        assert!(validate_pure_lan_ip("100.100.100.100").is_err());
        assert!(validate_pure_lan_ip("0.0.0.0").is_err());
        assert!(validate_pure_lan_ip("8.8.8.8").is_err());
        assert_eq!(
            validate_pure_lan_ip("192.168.1.20").unwrap(),
            Ipv4Addr::new(192, 168, 1, 20)
        );
        assert_eq!(
            validate_pure_lan_ip("192.168.1.10").unwrap(),
            Ipv4Addr::new(192, 168, 1, 10)
        );
    }

    #[test]
    fn default_config_roundtrips_with_strict_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = AppConfig::load_or_init(dir.path()).unwrap();
        assert_eq!(cfg.hosts.len(), 3);
        assert_eq!(cfg.preferred_brightness, 85);
        assert_eq!(cfg.input_provider_host_id, "win-desktop");
        assert_eq!(cfg.primary_ddc_controller_id, "macbook");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(AppConfig::config_path(dir.path()))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn swaps_ports_cleanly_when_mac_studio_moves_to_hdmi_and_windows_to_dp() {
        let mut cfg = AppConfig::default_home_setup("macbook", "tok-123".to_string());
        // Initial defaults: macbook=21 (USB-C), win-desktop=17 (HDMI), mac-studio=15 (DP, disabled)
        assert_eq!(cfg.hosts[0].monitor_input, 21);
        assert_eq!(cfg.hosts[1].monitor_input, 17);
        assert_eq!(cfg.hosts[2].monitor_input, 15);
        assert!(!cfg.hosts[2].enabled);

        // Now Mac Studio plugs into HDMI (17) and comes online on LAN!
        let changed = cfg.reconcile_detected_port("mac-studio", 17, true);
        assert!(changed);
        assert!(cfg.hosts[2].enabled);
        assert_eq!(cfg.hosts[2].monitor_input, 17); // Mac Studio owns HDMI (17)
        assert_eq!(cfg.hosts[1].monitor_input, 15); // Windows vacated to DP (15), zero collision!
        assert_eq!(cfg.hosts[0].monitor_input, 21); // MacBook stays USB-C (21)
    }

    #[test]
    fn reconcile_shared_settings_prefers_newer_rev_and_breaks_zero_rev_split_brain() {
        let mut mac = AppConfig::default_home_setup("macbook", "tok".to_string());
        mac.input_provider_host_id = "macbook".to_string();
        mac.ctrl_edge_switch_enabled = false;
        mac.allow_plain_slide_to_internal = false;

        let mut win = AppConfig::default_home_setup("win-desktop", "tok".to_string());
        win.input_provider_host_id = "win-desktop".to_string();
        win.ctrl_edge_switch_enabled = true;
        win.allow_plain_slide_to_internal = true;

        // 1. Equal rev==0 tie-break: macbook < win-desktop, so macbook bumps & asks win-desktop to update.
        let win_snap = win.shared_settings_snapshot();
        let mac_outcome = mac.reconcile_shared_settings(&win_snap, "win-desktop");
        assert!(mac_outcome.changed);
        assert!(mac_outcome.push_back_to_source);
        assert_eq!(mac.input_provider_host_id, "macbook");
        assert!(!mac.ctrl_edge_switch_enabled);
        assert!(mac.shared_settings_rev > 0);

        // 2. Windows receives higher rev from macbook and converges completely.
        let mac_snap = mac.shared_settings_snapshot();
        let win_outcome = win.reconcile_shared_settings(&mac_snap, "macbook");
        assert!(win_outcome.changed);
        assert!(win_outcome.deskflow_reload_needed);
        assert!(!win_outcome.push_back_to_source);
        assert_eq!(win.input_provider_host_id, "macbook");
        assert!(!win.ctrl_edge_switch_enabled);
        assert!(!win.allow_plain_slide_to_internal);
        assert_eq!(win.shared_settings_rev, mac.shared_settings_rev);

        // 3. Stale lower rev cannot overwrite macbook; tells sender to refresh instead.
        let mut stale_win = win.shared_settings_snapshot();
        stale_win.rev = 1;
        stale_win.ctrl_edge_switch_enabled = Some(true);
        let stale_outcome = mac.reconcile_shared_settings(&stale_win, "win-desktop");
        assert!(!stale_outcome.changed);
        assert!(stale_outcome.push_back_to_source);
        assert!(!mac.ctrl_edge_switch_enabled);

        // 4. Higher rev from Windows (e.g. user clicked toggle on Windows) syncs controller & height too.
        win.bump_shared_settings_rev();
        win.primary_ddc_controller_id = "mac-studio".to_string();
        win.set_internal_height_percent(78);
        let win_newer = win.shared_settings_snapshot();
        let mac_up = mac.reconcile_shared_settings(&win_newer, "win-desktop");
        assert!(mac_up.changed);
        assert!(mac_up.deskflow_reload_needed);
        assert_eq!(mac.primary_ddc_controller_id, "mac-studio");
        assert_eq!(mac.internal_height_percent(), 78);
    }
}
