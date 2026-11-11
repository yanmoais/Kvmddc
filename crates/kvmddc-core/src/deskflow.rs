use crate::config::{AppConfig, HostPlatform};
use anyhow::{bail, ensure, Context, Result};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct TlsIdentity {
    pub pem_path: PathBuf,
    /// Lowercase hex SHA-256 of X.509 DER certificate
    pub sha256_hex: String,
    /// Deskflow v2 fingerprint format: `v2:sha256:<hex>`
    pub v2_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeskflowLogEvent {
    Switched { from: String, to: String },
    ClientConnected,
    ClientDisconnected,
    EnteringScreen,
    LeavingScreen,
}

/// Maps internal slot `1..=3` to dedicated non-colliding function key used by Deskflow `switchToScreen`.
pub fn internal_switch_key_for_slot(slot: u8) -> Option<&'static str> {
    match slot {
        1 => Some("F18"),
        2 => Some("F19"),
        3 => Some("F20"),
        _ => None,
    }
}

/// Adapts hotkey modifier strings for Deskflow `server.conf` on macOS vs Windows/Linux.
/// Upstream Deskflow maps macOS `Command` -> `Alt` and `Option` -> `Super`.
pub fn format_deskflow_user_chord(server_platform: HostPlatform, slot: u8) -> Option<String> {
    if !(1..=9).contains(&slot) {
        return None;
    }
    let modifier = match server_platform {
        HostPlatform::Macos => "Control+Super",
        HostPlatform::Windows => "Control+Alt",
    };
    Some(format!("{modifier}+{slot}"))
}

pub fn sanitize_computer_section_key(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn load_or_generate_tls_identity(data_dir: &Path, common_name: &str) -> Result<TlsIdentity> {
    let df_dir = data_dir.join("deskflow");
    std::fs::create_dir_all(&df_dir)?;
    let pem_path = df_dir.join("identity.pem");

    if pem_path.exists() {
        let pem_text = std::fs::read_to_string(&pem_path)
            .with_context(|| format!("读取 TLS 证书失败: {}", pem_path.display()))?;
        let der = extract_first_cert_der_from_pem(&pem_text)?;
        let sha256_hex = hex::encode(Sha256::digest(&der));
        return Ok(TlsIdentity {
            pem_path,
            v2_fingerprint: format!("v2:sha256:{sha256_hex}"),
            sha256_hex,
        });
    }

    let certified = rcgen::generate_simple_self_signed(vec![common_name.to_string()])
        .context("生成自签名 TLS 证书失败")?;
    let cert_pem = certified.cert.pem();
    let key_pem = certified.key_pair.serialize_pem();
    let combined = format!("{cert_pem}{key_pem}");

    let tmp = pem_path.with_extension("pem.tmp");
    std::fs::write(&tmp, combined.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, &pem_path)?;

    let sha256_hex = hex::encode(Sha256::digest(certified.cert.der().as_ref()));
    Ok(TlsIdentity {
        pem_path,
        v2_fingerprint: format!("v2:sha256:{sha256_hex}"),
        sha256_hex,
    })
}

fn extract_first_cert_der_from_pem(pem: &str) -> Result<Vec<u8>> {
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";
    let start = pem.find(BEGIN).context("PEM 中缺少 CERTIFICATE 起始标记")? + BEGIN.len();
    let rest = &pem[start..];
    let end = rest.find(END).context("PEM 中缺少 CERTIFICATE 结束标记")?;
    let b64: String = rest[..end].chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("解码 PEM 证书内容失败")
}

/// Generates Deskflow 1.27 `server.conf` (links + options).
/// `active_plain_slide_peer_id` specifies the peer currently displayed on the 27" shared monitor
/// when the laptop's built-in screen is open alongside it; when `None` (e.g. laptop lid closed or
/// 27" monitor already displaying the laptop itself), no plain screen-edge links are emitted so
/// the cursor never escapes into an invisible desktop.
pub fn generate_server_conf(
    cfg: &AppConfig,
    active_plain_slide_peer_id: Option<&str>,
) -> Result<String> {
    cfg.validate()?;
    let _provider = cfg
        .hosts
        .iter()
        .find(|h| h.id == cfg.input_provider_host_id && h.enabled)
        .context("找不到键鼠提供端节点")?;

    let enabled = cfg.enabled_hosts();
    let mut out = String::new();
    out.push_str("section: links\n");

    let internal_host = enabled
        .iter()
        .copied()
        .find(|h| h.display_profile.has_internal_screen);
    let slide_peer = match (internal_host, active_plain_slide_peer_id) {
        (Some(ih), Some(pid)) if cfg.allow_plain_slide_to_internal && pid != ih.id => {
            enabled.iter().copied().find(|h| h.id == pid)
        }
        _ => None,
    };

    for host in &enabled {
        out.push_str(&format!("\t{}:\n", host.id));
        if let (Some(ih), Some(peer)) = (internal_host, slide_peer) {
            let pct = ih.display_profile.internal_height_percent.clamp(10, 100);
            if host.id == peer.id {
                out.push_str(&format!("\t\tright(0,{pct}) = {}(0,100)\n", ih.id));
            } else if host.id == ih.id {
                out.push_str(&format!("\t\tleft(0,100) = {}(0,{pct})\n", peer.id));
            }
        }
    }
    out.push_str("end\n\n");

    out.push_str("section: options\n");
    out.push_str(&format!("\tswitchDelay = {}\n", cfg.deskflow_switch_delay_ms));
    out.push_str("\trelativeMouseMoves = false\n");
    out.push_str("\tclipboardSharing = true\n");

    for host in &enabled {
        if let Some(fkey) = internal_switch_key_for_slot(host.hotkey_slot) {
            out.push_str(&format!("\tkeystroke({fkey}) = switchToScreen({})\n", host.id));
            out.push_str(&format!(
                "\tkeystroke(Control+{fkey}) = switchToScreen({})\n",
                host.id
            ));
            out.push_str(&format!(
                "\tkeystroke(Control+Alt+{fkey}) = switchToScreen({})\n",
                host.id
            ));
            out.push_str(&format!(
                "\tkeystroke(Control+Super+{fkey}) = switchToScreen({})\n",
                host.id
            ));
        }
    }
    out.push_str("end\n");
    Ok(out)
}

/// Writes `Deskflow.conf`, `server.conf` (if provider), and trusted peer fingerprint files.
/// Returns `(deskflow_conf_path, is_server)`.
pub fn write_deskflow_runtime_files(
    cfg: &AppConfig,
    data_dir: &Path,
    identity: &TlsIdentity,
    active_plain_slide_peer_id: Option<&str>,
) -> Result<(PathBuf, bool)> {
    cfg.validate()?;
    let local = cfg.local_host()?;
    let provider = cfg
        .hosts
        .iter()
        .find(|h| h.id == cfg.input_provider_host_id && h.enabled)
        .context("找不到已启用的键鼠提供端主机")?;

    let is_server = local.id == provider.id;
    let df_dir = data_dir.join("deskflow");
    let tls_dir = df_dir.join("tls");
    std::fs::create_dir_all(&tls_dir)?;

    // Mirror trusted fingerprints to both local runtime dir and OS QStandardPaths Deskflow dir
    let mut tls_dirs = vec![tls_dir];
    for extra in system_deskflow_tls_dirs() {
        if std::fs::create_dir_all(&extra).is_ok() {
            tls_dirs.push(extra);
        }
    }

    let mut peer_fps = Vec::new();
    for h in cfg.enabled_hosts() {
        if h.id != local.id {
            if let Some(fp) = &h.tls_fingerprint {
                let trimmed = fp.trim();
                if !trimmed.is_empty() {
                    let clean_hex = trimmed.strip_prefix("v2:sha256:").unwrap_or(trimmed);
                    peer_fps.push(format!("v2:sha256:{clean_hex}"));
                    peer_fps.push(clean_hex.to_string());
                }
            }
        }
    }
    let has_peer_fps = !peer_fps.is_empty();
    let fp_payload = if has_peer_fps {
        format!("{}\n", peer_fps.join("\n"))
    } else {
        String::new()
    };

    for td in &tls_dirs {
        if is_server {
            std::fs::write(td.join("trusted-clients"), fp_payload.as_bytes())?;
        } else if let Some(srv_fp) = &provider.tls_fingerprint {
            let clean = srv_fp.trim().strip_prefix("v2:sha256:").unwrap_or(srv_fp.trim());
            if !clean.is_empty() {
                let srv_payload = format!("v2:sha256:{clean}\n{clean}\n");
                std::fs::write(td.join("trusted-servers"), srv_payload.as_bytes())?;
            }
        }
    }

    let server_conf_path = df_dir.join("server.conf");
    if is_server {
        let sconf = generate_server_conf(cfg, active_plain_slide_peer_id)?;
        std::fs::write(&server_conf_path, sconf.as_bytes())?;
    }

    let check_fp = if is_server {
        has_peer_fps
    } else {
        provider
            .tls_fingerprint
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
    };

    let cert_norm = identity.pem_path.to_string_lossy().replace('\\', "/");
    let sconf_norm = server_conf_path.to_string_lossy().replace('\\', "/");

    let mut ini = String::new();
    ini.push_str("[core]\n");
    ini.push_str(&format!("computerName={}\n", local.id));
    ini.push_str(&format!("screenName={}\n", local.id));
    if is_server {
        ini.push_str(&format!("interface={}\n", local.lan_ip));
    }
    ini.push_str(&format!("port={}\n", cfg.deskflow_port));
    ini.push_str("preventSleep=false\n");
    ini.push_str("useHooks=true\n\n");

    if is_server {
        for h in cfg.enabled_hosts() {
            let sec = sanitize_computer_section_key(&h.id);
            ini.push_str(&format!("[computer_{sec}]\n"));
            ini.push_str(&format!("name={}\n", h.id));
            if cfg.unify_cross_os_modifiers && h.platform != provider.platform {
                // Deskflow 1.27 modifierArray enum:
                // 0=Shift, 1=Ctrl, 2=Alt(macOS Command), 3=Meta, 4=Super(macOS Option / Win key), 5=AltGr
                let mods: [u8; 6] = match (provider.platform, h.platform) {
                    // Windows keyboard -> macOS target: Ctrl->Command(2), Alt->Option(4), Win(Super)->Control(1)
                    (HostPlatform::Windows, HostPlatform::Macos) => [0, 2, 4, 3, 1, 5],
                    // macOS keyboard -> Windows target: Command(Alt)->Ctrl(1), Option(Super)->Alt(2), Control->Win(4)
                    (HostPlatform::Macos, HostPlatform::Windows) => [0, 4, 1, 3, 2, 5],
                    _ => [0, 1, 2, 3, 4, 5],
                };
                ini.push_str("modifierArray\\size=6\n");
                for (i, m) in mods.iter().enumerate() {
                    ini.push_str(&format!("modifierArray\\{}\\modifier={}\n", i + 1, m));
                }
            }
            ini.push('\n');
        }
        ini.push_str("[server]\n");
        ini.push_str("externalConfig=true\n");
        ini.push_str(&format!("externalConfigFile={sconf_norm}\n"));
        ini.push_str("enableClipboard=true\n");
        ini.push_str("enableHeartbeat=true\n");
        ini.push_str("heartbeat=5000\n\n");
    } else {
        ini.push_str("[client]\n");
        ini.push_str(&format!("remoteHost={}\n", provider.lan_ip));
        ini.push_str("dynamicConnectionInterval=false\n\n");
    }

    ini.push_str("[security]\n");
    ini.push_str("tlsEnabled=true\n");
    ini.push_str(&format!("certificate={cert_norm}\n"));
    ini.push_str(&format!("checkPeerFingerprints={check_fp}\n\n"));

    ini.push_str("[log]\n");
    ini.push_str("level=INFO\n");
    ini.push_str("toFile=false\n");

    let conf_path = df_dir.join("Deskflow.conf");
    std::fs::write(&conf_path, ini.as_bytes())?;
    Ok((conf_path, is_server))
}

fn system_deskflow_tls_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(target_os = "macos")]
    if let Ok(home) = std::env::var("HOME") {
        let hp = PathBuf::from(home);
        dirs.push(hp.join("Library/Deskflow/tls"));
        dirs.push(hp.join("Library/Application Support/Deskflow/tls"));
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local_app).join("Deskflow\\tls"));
        }
        if let Ok(roaming) = std::env::var("APPDATA") {
            dirs.push(PathBuf::from(roaming).join("Deskflow\\tls"));
        }
    }
    dirs
}

/// Creates a short isolated Unix TMPDIR (< 48 chars) to avoid stale Qt IPC semaphores and Unix socket 104-byte limit.
pub fn create_isolated_short_tmpdir() -> Result<PathBuf> {
    let short_id = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let base = if cfg!(target_os = "windows") {
        std::env::temp_dir()
    } else {
        PathBuf::from("/tmp")
    };
    let dir = base.join(format!("kvdf-{short_id}"));
    std::fs::create_dir_all(&dir)?;
    ensure!(
        dir.to_string_lossy().len() < 80,
        "Deskflow 临时目录路径过长: {}",
        dir.display()
    );
    Ok(dir)
}

pub fn resolve_deskflow_binary(cfg: &AppConfig) -> Result<PathBuf> {
    if let Some(custom) = &cfg.deskflow_binary_override {
        let p = PathBuf::from(custom);
        if p.exists() {
            return Ok(p);
        }
        bail!("配置的 Deskflow 路径不存在: {}", p.display());
    }

    let mut candidates = Vec::new();
    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from(
            "/Applications/Deskflow.app/Contents/MacOS/deskflow-core",
        ));
        candidates.push(PathBuf::from("/opt/homebrew/bin/deskflow-core"));
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
            candidates.push(
                PathBuf::from(&local_app)
                    .join("Programs\\Deskflow\\deskflow-1.27.0-win-x64-portable\\deskflow-core.exe"),
            );
            candidates.push(PathBuf::from(&local_app).join("Programs\\Deskflow\\deskflow-core.exe"));
        }
        candidates.push(PathBuf::from("C:\\Program Files\\Deskflow\\deskflow-core.exe"));
    }

    for c in candidates {
        if c.exists() {
            return Ok(c);
        }
    }
    bail!("未找到 deskflow-core 可执行文件，请检查安装路径")
}

pub fn parse_deskflow_log_line(line: &str) -> Option<DeskflowLogEvent> {
    if let Some(idx) = line.find("switch from \"") {
        let rest = &line[idx + "switch from \"".len()..];
        let end_from = rest.find('"')?;
        let from = rest[..end_from].to_string();
        let after_from = &rest[end_from + 1..];
        let to_prefix = " to \"";
        let to_idx = after_from.find(to_prefix)?;
        let rest_to = &after_from[to_idx + to_prefix.len()..];
        let end_to = rest_to.find('"')?;
        let to = rest_to[..end_to].to_string();
        return Some(DeskflowLogEvent::Switched { from, to });
    }
    if line.contains("accepted client connection")
        || line.contains("accepted secure socket")
        || line.contains("has connected")
        || line.contains("connected to server")
        || line.contains("connected to secure socket")
    {
        return Some(DeskflowLogEvent::ClientConnected);
    }
    if line.contains("disconnected from server") || line.contains("has disconnected") || line.contains("tls connection closed") {
        return Some(DeskflowLogEvent::ClientDisconnected);
    }
    if line.contains("entering screen") {
        return Some(DeskflowLogEvent::EnteringScreen);
    }
    if line.contains("leaving screen") {
        return Some(DeskflowLogEvent::LeavingScreen);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn darwin_modifier_uses_super_instead_of_alt() {
        assert_eq!(
            format_deskflow_user_chord(HostPlatform::Macos, 1).as_deref(),
            Some("Control+Super+1")
        );
        assert_eq!(
            format_deskflow_user_chord(HostPlatform::Windows, 2).as_deref(),
            Some("Control+Alt+2")
        );
    }

    #[test]
    fn generates_stable_tls_identity_and_server_conf() {
        let dir = tempfile::tempdir().unwrap();
        let id1 = load_or_generate_tls_identity(dir.path(), "win-desktop").unwrap();
        let id2 = load_or_generate_tls_identity(dir.path(), "win-desktop").unwrap();
        assert_eq!(id1.sha256_hex.len(), 64);
        assert_eq!(id1.v2_fingerprint, id2.v2_fingerprint);

        let cfg = AppConfig::default_home_setup("win-desktop", "tok123".to_string());
        let sconf = generate_server_conf(&cfg, Some("win-desktop")).unwrap();
        assert!(sconf.contains("right(0,81) = macbook(0,100)"));
        assert!(sconf.contains("left(0,100) = win-desktop(0,81)"));
        assert!(sconf.contains("keystroke(F18) = switchToScreen(macbook)"));
        assert!(sconf.contains("keystroke(F19) = switchToScreen(win-desktop)"));

        let locked = generate_server_conf(&cfg, None).unwrap();
        assert!(!locked.contains("right(0,81)"));
        assert!(!locked.contains("left(0,100)"));
        assert!(locked.contains("keystroke(F19) = switchToScreen(win-desktop)"));

        // When 27" monitor is displaying macbook itself, edges must stay locked
        let self_mon = generate_server_conf(&cfg, Some("macbook")).unwrap();
        assert!(!self_mon.contains("right("));
        assert!(!self_mon.contains("left("));

        let (conf_path, is_srv) =
            write_deskflow_runtime_files(&cfg, dir.path(), &id1, Some("win-desktop")).unwrap();
        assert!(is_srv);
        let ini_text = std::fs::read_to_string(conf_path).unwrap();
        // Windows provider -> macbook client maps Ctrl(idx 2)->Command(2), Alt(idx 3)->Option(4), Super(idx 5)->Ctrl(1)
        assert!(ini_text.contains("[computer_macbook]\nname=macbook\nmodifierArray\\size=6\nmodifierArray\\1\\modifier=0\nmodifierArray\\2\\modifier=2\nmodifierArray\\3\\modifier=4"));
        // Same-OS win-desktop has no remapping array
        assert!(ini_text.contains("[computer_win-desktop]\nname=win-desktop\n\n"));
    }

    #[test]
    fn macbook_as_provider_still_generates_bidirectional_edge_links() {
        let mut cfg = AppConfig::default_home_setup("macbook", "tok123".to_string());
        cfg.input_provider_host_id = "macbook".to_string();
        let sconf = generate_server_conf(&cfg, Some("win-desktop")).unwrap();
        assert!(sconf.contains("right(0,81) = macbook(0,100)"));
        assert!(sconf.contains("left(0,100) = win-desktop(0,81)"));

        // Clamshell closed -> None -> zero edge links
        let clamshell = generate_server_conf(&cfg, None).unwrap();
        assert!(!clamshell.contains("left("));
        assert!(!clamshell.contains("right("));
    }

    #[test]
    fn parses_deskflow_switch_and_connection_log_lines() {
        let line = "[2026-11-09T00:36:56.343] INFO: switch from \"win-desktop\" to \"macbook\" at 0,520";
        assert_eq!(
            parse_deskflow_log_line(line),
            Some(DeskflowLogEvent::Switched {
                from: "win-desktop".to_string(),
                to: "macbook".to_string()
            })
        );
        assert_eq!(
            parse_deskflow_log_line("[2026-11-09T00:36:56.343] INFO: accepted client connection"),
            Some(DeskflowLogEvent::ClientConnected)
        );
    }
}
