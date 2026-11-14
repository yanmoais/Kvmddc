use crate::config::{validate_pure_lan_ip, AppConfig, HostPlatform, SharedClusterSettings};
use crate::db::{SwitchRequestRow, SwitchSource};
use anyhow::{bail, ensure, Context, Result};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DisplayModeDto {
    pub mode_id: i32,
    pub width: u32,
    pub height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub hz: u32,
    pub hidpi: bool,
    pub active: bool,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerStatusResponse {
    pub host_id: String,
    pub platform: HostPlatform,
    pub is_input_provider: bool,
    pub is_ddc_controller: bool,
    pub deskflow_running: bool,
    pub deskflow_connected: bool,
    pub active_cursor_host: Option<String>,
    pub observed_monitor_input: Option<u8>,
    pub current_brightness: Option<u8>,
    pub edge_locked: bool,
    pub secure_input_active: bool,
    pub tls_fingerprint: String,
    pub active_switch: Option<SwitchRequestRow>,
    #[serde(default)]
    pub detected_monitor_input: Option<u8>,
    #[serde(default)]
    pub detected_port_label: Option<String>,
    #[serde(default)]
    pub detected_mode_summary: Option<String>,
    #[serde(default)]
    pub detected_monitor_model: Option<String>,
    #[serde(default)]
    pub display_sleeping: bool,
    #[serde(default)]
    pub current_volume: Option<u8>,
    #[serde(default)]
    pub audio_muted: Option<bool>,
    #[serde(default)]
    pub current_color_preset: Option<u8>,
    #[serde(default)]
    pub display_modes: Vec<DisplayModeDto>,
    #[serde(default)]
    pub clamshell_closed: bool,
    #[serde(default)]
    pub shared_settings_rev: u64,
    #[serde(default)]
    pub input_provider_host_id: Option<String>,
    #[serde(default)]
    pub primary_ddc_controller_id: Option<String>,
    #[serde(default)]
    pub deskflow_enabled: Option<bool>,
    #[serde(default)]
    pub ctrl_edge_switch_enabled: Option<bool>,
    #[serde(default)]
    pub allow_plain_slide_to_internal: Option<bool>,
    #[serde(default)]
    pub unify_cross_os_modifiers: Option<bool>,
    #[serde(default)]
    pub auto_switch_input_provider_on_hotplug: Option<bool>,
    #[serde(default)]
    pub internal_height_percent: Option<u8>,
}

impl PeerStatusResponse {
    pub fn to_shared_settings(&self) -> SharedClusterSettings {
        SharedClusterSettings {
            rev: self.shared_settings_rev,
            input_provider_host_id: self.input_provider_host_id.clone(),
            primary_ddc_controller_id: self.primary_ddc_controller_id.clone(),
            deskflow_enabled: self.deskflow_enabled,
            ctrl_edge_switch_enabled: self.ctrl_edge_switch_enabled,
            allow_plain_slide_to_internal: self.allow_plain_slide_to_internal,
            unify_cross_os_modifiers: self.unify_cross_os_modifiers,
            auto_switch_input_provider_on_hotplug: self.auto_switch_input_provider_on_hotplug,
            internal_height_percent: self.internal_height_percent,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSwitchRequest {
    pub request_key: String,
    pub source: SwitchSource,
    pub from_host: Option<String>,
    pub to_host: String,
    pub target_input: u8,
    pub sync_input_focus: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionAck {
    pub ok: bool,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusInputRequest {
    pub target_host_id: String,
    pub hotkey_slot: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrightnessRequest {
    pub brightness: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeRequest {
    pub volume: Option<u8>,
    pub muted: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorPresetRequest {
    pub preset: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayModeRequest {
    pub mode_id: i32,
    pub width: u32,
    pub height: u32,
    pub hz: u32,
    #[serde(default)]
    pub hidpi: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairPeerRequest {
    pub host_id: String,
    pub lan_ip: String,
    pub tls_fingerprint: String,
    pub mac_address: Option<String>,
    #[serde(default)]
    pub shared_settings_rev: Option<u64>,
    #[serde(default)]
    pub input_provider_host_id: Option<String>,
    #[serde(default)]
    pub primary_ddc_controller_id: Option<String>,
    #[serde(default)]
    pub deskflow_enabled: Option<bool>,
    #[serde(default)]
    pub ctrl_edge_switch_enabled: Option<bool>,
    #[serde(default)]
    pub allow_plain_slide_to_internal: Option<bool>,
    #[serde(default)]
    pub unify_cross_os_modifiers: Option<bool>,
    #[serde(default)]
    pub auto_switch_input_provider_on_hotplug: Option<bool>,
    #[serde(default)]
    pub internal_height_percent: Option<u8>,
}

impl PairPeerRequest {
    pub fn announce_only(
        host_id: String,
        lan_ip: String,
        tls_fingerprint: String,
    ) -> Self {
        Self {
            host_id,
            lan_ip,
            tls_fingerprint,
            mac_address: None,
            shared_settings_rev: None,
            input_provider_host_id: None,
            primary_ddc_controller_id: None,
            deskflow_enabled: None,
            ctrl_edge_switch_enabled: None,
            allow_plain_slide_to_internal: None,
            unify_cross_os_modifiers: None,
            auto_switch_input_provider_on_hotplug: None,
            internal_height_percent: None,
        }
    }

    pub fn from_config(cfg: &AppConfig, lan_ip: String, tls_fingerprint: String) -> Self {
        Self {
            host_id: cfg.local_host_id.clone(),
            lan_ip,
            tls_fingerprint,
            mac_address: None,
            shared_settings_rev: Some(cfg.shared_settings_rev),
            input_provider_host_id: Some(cfg.input_provider_host_id.clone()),
            primary_ddc_controller_id: Some(cfg.primary_ddc_controller_id.clone()),
            deskflow_enabled: Some(cfg.deskflow_enabled),
            ctrl_edge_switch_enabled: Some(cfg.ctrl_edge_switch_enabled),
            allow_plain_slide_to_internal: Some(cfg.allow_plain_slide_to_internal),
            unify_cross_os_modifiers: Some(cfg.unify_cross_os_modifiers),
            auto_switch_input_provider_on_hotplug: Some(cfg.auto_switch_input_provider_on_hotplug),
            internal_height_percent: Some(cfg.internal_height_percent()),
        }
    }

    pub fn to_shared_settings(&self) -> SharedClusterSettings {
        SharedClusterSettings {
            rev: self.shared_settings_rev.unwrap_or(0),
            input_provider_host_id: self.input_provider_host_id.clone(),
            primary_ddc_controller_id: self.primary_ddc_controller_id.clone(),
            deskflow_enabled: self.deskflow_enabled,
            ctrl_edge_switch_enabled: self.ctrl_edge_switch_enabled,
            allow_plain_slide_to_internal: self.allow_plain_slide_to_internal,
            unify_cross_os_modifiers: self.unify_cross_os_modifiers,
            auto_switch_input_provider_on_hotplug: self.auto_switch_input_provider_on_hotplug,
            internal_height_percent: self.internal_height_percent,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairPeerResponse {
    pub host_id: String,
    pub tls_fingerprint: String,
    pub mac_address: Option<String>,
}

pub fn sanitize_transfer_rel_path(rel: &str) -> Result<PathBuf> {
    let normalized = rel.trim().replace('\\', "/");
    ensure!(!normalized.is_empty(), "传输文件相对路径不能为空");
    ensure!(!normalized.contains('\0'), "非法文件路径字符");
    let p = Path::new(&normalized);
    let mut clean = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::Normal(seg) => {
                let s = seg.to_string_lossy();
                ensure!(
                    !s.is_empty() && !s.contains(':'),
                    "拒绝非法路径段: {s}"
                );
                clean.push(seg);
            }
            _ => bail!("拒绝包含越界组件的文件路径: {rel}"),
        }
    }
    ensure!(!clean.as_os_str().is_empty(), "解析后文件路径为空");
    Ok(clean)
}

pub fn sha256_hex_bytes(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub trait ControlPlaneHandler: Send + Sync + 'static {
    fn shared_token(&self) -> String;
    fn get_status(&self) -> BoxFuture<'_, Result<PeerStatusResponse>>;
    fn handle_switch(&self, req: RemoteSwitchRequest) -> BoxFuture<'_, Result<ActionAck>>;
    fn handle_focus_input(&self, req: FocusInputRequest) -> BoxFuture<'_, Result<ActionAck>>;
    fn handle_brightness(&self, req: BrightnessRequest) -> BoxFuture<'_, Result<ActionAck>>;
    fn handle_volume(&self, req: VolumeRequest) -> BoxFuture<'_, Result<ActionAck>>;
    fn handle_color_preset(&self, req: ColorPresetRequest) -> BoxFuture<'_, Result<ActionAck>>;
    fn handle_display_mode(&self, req: DisplayModeRequest) -> BoxFuture<'_, Result<ActionAck>>;
    fn handle_display_sleep(&self) -> BoxFuture<'_, Result<ActionAck>>;
    fn handle_wake(&self) -> BoxFuture<'_, Result<ActionAck>>;
    fn handle_pair(&self, req: PairPeerRequest) -> BoxFuture<'_, Result<PairPeerResponse>>;
    fn handle_receive_file(
        &self,
        rel_path: PathBuf,
        data: Bytes,
    ) -> BoxFuture<'_, Result<ActionAck>>;
    fn handle_receive_clipboard_png(&self, png_data: Bytes) -> BoxFuture<'_, Result<ActionAck>>;
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn check_bearer(headers: &HeaderMap, expected_token: &str) -> std::result::Result<(), StatusCode> {
    let Some(val) = headers.get(axum::http::header::AUTHORIZATION) else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let Ok(raw) = val.to_str() else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let Some(provided) = raw.strip_prefix("Bearer ") else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    if expected_token.is_empty() || !constant_time_eq(provided.as_bytes(), expected_token.as_bytes()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}

pub fn build_control_router(handler: Arc<dyn ControlPlaneHandler>) -> Router {
    let transfer_routes = Router::new()
        .route("/v1/transfer-file", post(route_transfer_file))
        .route("/v1/clipboard-image", post(route_clipboard_image))
        .layer(DefaultBodyLimit::max(512 * 1024 * 1024));

    Router::new()
        .route("/v1/status", get(route_status))
        .route("/v1/switch", post(route_switch))
        .route("/v1/focus-input", post(route_focus_input))
        .route("/v1/brightness", post(route_brightness))
        .route("/v1/volume", post(route_volume))
        .route("/v1/color-preset", post(route_color_preset))
        .route("/v1/display-mode", post(route_display_mode))
        .route("/v1/display-sleep", post(route_display_sleep))
        .route("/v1/wake", post(route_wake))
        .route("/v1/pair", post(route_pair))
        .merge(transfer_routes)
        .with_state(handler)
}

pub async fn bind_pure_lan_listener(lan_ip: &str, port: u16) -> Result<tokio::net::TcpListener> {
    let ipv4: Ipv4Addr = validate_pure_lan_ip(lan_ip)?;
    let addr = SocketAddr::V4(SocketAddrV4::new(ipv4, port));
    tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("绑定局域网控制端口失败: {addr}"))
}

async fn route_status(
    State(h): State<Arc<dyn ControlPlaneHandler>>,
    headers: HeaderMap,
) -> std::result::Result<Json<PeerStatusResponse>, (StatusCode, String)> {
    check_bearer(&headers, &h.shared_token()).map_err(|c| (c, "鉴权失败".to_string()))?;
    h.get_status()
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn route_switch(
    State(h): State<Arc<dyn ControlPlaneHandler>>,
    headers: HeaderMap,
    Json(req): Json<RemoteSwitchRequest>,
) -> std::result::Result<Json<ActionAck>, (StatusCode, String)> {
    check_bearer(&headers, &h.shared_token()).map_err(|c| (c, "鉴权失败".to_string()))?;
    h.handle_switch(req)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

async fn route_focus_input(
    State(h): State<Arc<dyn ControlPlaneHandler>>,
    headers: HeaderMap,
    Json(req): Json<FocusInputRequest>,
) -> std::result::Result<Json<ActionAck>, (StatusCode, String)> {
    check_bearer(&headers, &h.shared_token()).map_err(|c| (c, "鉴权失败".to_string()))?;
    h.handle_focus_input(req)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

async fn route_brightness(
    State(h): State<Arc<dyn ControlPlaneHandler>>,
    headers: HeaderMap,
    Json(req): Json<BrightnessRequest>,
) -> std::result::Result<Json<ActionAck>, (StatusCode, String)> {
    check_bearer(&headers, &h.shared_token()).map_err(|c| (c, "鉴权失败".to_string()))?;
    h.handle_brightness(req)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

async fn route_volume(
    State(h): State<Arc<dyn ControlPlaneHandler>>,
    headers: HeaderMap,
    Json(req): Json<VolumeRequest>,
) -> std::result::Result<Json<ActionAck>, (StatusCode, String)> {
    check_bearer(&headers, &h.shared_token()).map_err(|c| (c, "鉴权失败".to_string()))?;
    h.handle_volume(req)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

async fn route_color_preset(
    State(h): State<Arc<dyn ControlPlaneHandler>>,
    headers: HeaderMap,
    Json(req): Json<ColorPresetRequest>,
) -> std::result::Result<Json<ActionAck>, (StatusCode, String)> {
    check_bearer(&headers, &h.shared_token()).map_err(|c| (c, "鉴权失败".to_string()))?;
    h.handle_color_preset(req)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

async fn route_display_mode(
    State(h): State<Arc<dyn ControlPlaneHandler>>,
    headers: HeaderMap,
    Json(req): Json<DisplayModeRequest>,
) -> std::result::Result<Json<ActionAck>, (StatusCode, String)> {
    check_bearer(&headers, &h.shared_token()).map_err(|c| (c, "鉴权失败".to_string()))?;
    h.handle_display_mode(req)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

async fn route_transfer_file(
    State(h): State<Arc<dyn ControlPlaneHandler>>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<Json<ActionAck>, (StatusCode, String)> {
    check_bearer(&headers, &h.shared_token()).map_err(|c| (c, "鉴权失败".to_string()))?;
    let b64_path = headers
        .get("x-rel-path-b64")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "缺少 x-rel-path-b64 头".to_string()))?;
    let expected_sha = headers
        .get("x-sha256")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "缺少 x-sha256 头".to_string()))?;

    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(b64_path)
        .map_err(|_| (StatusCode::BAD_REQUEST, "文件名编码无效".to_string()))?;
    let rel_str = String::from_utf8(decoded)
        .map_err(|_| (StatusCode::BAD_REQUEST, "文件名非 UTF-8".to_string()))?;
    let clean_rel = sanitize_transfer_rel_path(&rel_str)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let actual_sha = sha256_hex_bytes(&body);
    if !constant_time_eq(actual_sha.as_bytes(), expected_sha.as_bytes()) {
        return Err((StatusCode::BAD_REQUEST, "SHA-256 校验不一致".to_string()));
    }

    h.handle_receive_file(clean_rel, body)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn route_clipboard_image(
    State(h): State<Arc<dyn ControlPlaneHandler>>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<Json<ActionAck>, (StatusCode, String)> {
    check_bearer(&headers, &h.shared_token()).map_err(|c| (c, "鉴权失败".to_string()))?;
    if body.is_empty() || body.len() > 32 * 1024 * 1024 {
        return Err((StatusCode::BAD_REQUEST, "截图大小超出限制".to_string()));
    }
    if let Some(expected_sha) = headers.get("x-sha256").and_then(|v| v.to_str().ok()) {
        let actual_sha = sha256_hex_bytes(&body);
        if !constant_time_eq(actual_sha.as_bytes(), expected_sha.as_bytes()) {
            return Err((StatusCode::BAD_REQUEST, "截图 SHA-256 校验不一致".to_string()));
        }
    }
    h.handle_receive_clipboard_png(body)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn route_display_sleep(
    State(h): State<Arc<dyn ControlPlaneHandler>>,
    headers: HeaderMap,
) -> std::result::Result<Json<ActionAck>, (StatusCode, String)> {
    check_bearer(&headers, &h.shared_token()).map_err(|c| (c, "鉴权失败".to_string()))?;
    h.handle_display_sleep()
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn route_wake(
    State(h): State<Arc<dyn ControlPlaneHandler>>,
    headers: HeaderMap,
) -> std::result::Result<Json<ActionAck>, (StatusCode, String)> {
    check_bearer(&headers, &h.shared_token()).map_err(|c| (c, "鉴权失败".to_string()))?;
    h.handle_wake()
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn route_pair(
    State(h): State<Arc<dyn ControlPlaneHandler>>,
    headers: HeaderMap,
    Json(req): Json<PairPeerRequest>,
) -> std::result::Result<Json<PairPeerResponse>, (StatusCode, String)> {
    check_bearer(&headers, &h.shared_token()).map_err(|c| (c, "鉴权失败".to_string()))?;
    h.handle_pair(req)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

#[derive(Clone)]
pub struct LanControlClient {
    http: reqwest::Client,
    transfer_http: reqwest::Client,
}

impl LanControlClient {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_millis(1200))
            .timeout(Duration::from_millis(3500))
            .build()
            .context("构建局域网直连 HTTP 客户端失败")?;
        let transfer_http = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_millis(2000))
            .timeout(Duration::from_secs(300))
            .build()
            .context("构建局域网大文件直传客户端失败")?;
        Ok(Self {
            http,
            transfer_http,
        })
    }

    fn base_url(ip: &str, port: u16) -> Result<String> {
        validate_pure_lan_ip(ip)?;
        ensure!(port >= 1024, "端口号非法");
        Ok(format!("http://{ip}:{port}"))
    }

    pub async fn status(&self, ip: &str, port: u16, token: &str) -> Result<PeerStatusResponse> {
        let url = format!("{}/v1/status", Self::base_url(ip, port)?);
        let resp = self.http.get(url).bearer_auth(token).send().await?;
        if !resp.status().is_success() {
            bail!("对端状态接口返回 HTTP {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn request_switch(
        &self,
        ip: &str,
        port: u16,
        token: &str,
        req: &RemoteSwitchRequest,
    ) -> Result<ActionAck> {
        let url = format!("{}/v1/switch", Self::base_url(ip, port)?);
        let resp = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(req)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("远程切屏失败 HTTP {status}: {text}");
        }
        Ok(resp.json().await?)
    }

    pub async fn focus_input(
        &self,
        ip: &str,
        port: u16,
        token: &str,
        req: &FocusInputRequest,
    ) -> Result<ActionAck> {
        let url = format!("{}/v1/focus-input", Self::base_url(ip, port)?);
        let resp = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(req)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("切换键鼠焦点失败 HTTP {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn set_brightness(
        &self,
        ip: &str,
        port: u16,
        token: &str,
        brightness: u8,
    ) -> Result<ActionAck> {
        let url = format!("{}/v1/brightness", Self::base_url(ip, port)?);
        let resp = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(&BrightnessRequest { brightness })
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("远程调节亮度失败 HTTP {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn display_sleep(&self, ip: &str, port: u16, token: &str) -> Result<ActionAck> {
        let url = format!("{}/v1/display-sleep", Self::base_url(ip, port)?);
        let resp = self.http.post(url).bearer_auth(token).send().await?;
        if !resp.status().is_success() {
            bail!("远程熄屏待机失败 HTTP {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn wake(&self, ip: &str, port: u16, token: &str) -> Result<ActionAck> {
        let url = format!("{}/v1/wake", Self::base_url(ip, port)?);
        let resp = self.http.post(url).bearer_auth(token).send().await?;
        if !resp.status().is_success() {
            bail!("远程亮屏唤醒失败 HTTP {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn pair(
        &self,
        ip: &str,
        port: u16,
        token: &str,
        req: &PairPeerRequest,
    ) -> Result<PairPeerResponse> {
        let url = format!("{}/v1/pair", Self::base_url(ip, port)?);
        let resp = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(req)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("节点握手配对失败 HTTP {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn set_volume(
        &self,
        ip: &str,
        port: u16,
        token: &str,
        req: &VolumeRequest,
    ) -> Result<ActionAck> {
        let url = format!("{}/v1/volume", Self::base_url(ip, port)?);
        let resp = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(req)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("远程调节音量失败 HTTP {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn set_color_preset(
        &self,
        ip: &str,
        port: u16,
        token: &str,
        preset: u8,
    ) -> Result<ActionAck> {
        let url = format!("{}/v1/color-preset", Self::base_url(ip, port)?);
        let resp = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(&ColorPresetRequest { preset })
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("远程切换色温失败 HTTP {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn set_display_mode(
        &self,
        ip: &str,
        port: u16,
        token: &str,
        req: &DisplayModeRequest,
    ) -> Result<ActionAck> {
        let url = format!("{}/v1/display-mode", Self::base_url(ip, port)?);
        let resp = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(req)
            .send()
            .await?;
        if !resp.status().is_success() {
            let txt = resp.text().await.unwrap_or_default();
            bail!("远程切换分辨率失败: {txt}");
        }
        Ok(resp.json().await?)
    }

    pub async fn transfer_file(
        &self,
        ip: &str,
        port: u16,
        token: &str,
        rel_path: &str,
        data: Vec<u8>,
    ) -> Result<ActionAck> {
        let _ = sanitize_transfer_rel_path(rel_path)?;
        let url = format!("{}/v1/transfer-file", Self::base_url(ip, port)?);
        let b64_path =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rel_path.as_bytes());
        let sha = sha256_hex_bytes(&data);
        let resp = self
            .transfer_http
            .post(url)
            .bearer_auth(token)
            .header("x-rel-path-b64", b64_path)
            .header("x-sha256", sha)
            .body(data)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let txt = resp.text().await.unwrap_or_default();
            bail!("局域网传文件失败 HTTP {status}: {txt}");
        }
        Ok(resp.json().await?)
    }

    pub async fn send_clipboard_png(
        &self,
        ip: &str,
        port: u16,
        token: &str,
        png_bytes: Vec<u8>,
    ) -> Result<ActionAck> {
        let url = format!("{}/v1/clipboard-image", Self::base_url(ip, port)?);
        let sha = sha256_hex_bytes(&png_bytes);
        let resp = self
            .transfer_http
            .post(url)
            .bearer_auth(token)
            .header("x-sha256", sha)
            .body(png_bytes)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("同步剪贴板截图失败 HTTP {}", resp.status());
        }
        Ok(resp.json().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockHandler;

    impl ControlPlaneHandler for MockHandler {
        fn shared_token(&self) -> String {
            "secret-lan-token".to_string()
        }

        fn get_status(&self) -> BoxFuture<'_, Result<PeerStatusResponse>> {
            Box::pin(async {
                Ok(PeerStatusResponse {
                    host_id: "macbook".to_string(),
                    platform: HostPlatform::Macos,
                    is_input_provider: false,
                    is_ddc_controller: true,
                    deskflow_running: true,
                    deskflow_connected: true,
                    active_cursor_host: Some("win-desktop".to_string()),
                    observed_monitor_input: Some(17),
                    current_brightness: Some(85),
                    edge_locked: false,
                    secure_input_active: false,
                    tls_fingerprint: "v2:sha256:abcd".to_string(),
                    active_switch: None,
                    detected_monitor_input: Some(21),
                    detected_port_label: Some("USB-C".to_string()),
                    detected_mode_summary: Some("5120×2880 · 75Hz".to_string()),
                    detected_monitor_model: Some("27E2N6900RW".to_string()),
                    display_sleeping: false,
                    current_volume: Some(75),
                    audio_muted: Some(false),
                    current_color_preset: Some(5),
                    display_modes: Vec::new(),
                    clamshell_closed: false,
                    shared_settings_rev: 0,
                    input_provider_host_id: Some("win-desktop".to_string()),
                    primary_ddc_controller_id: Some("macbook".to_string()),
                    deskflow_enabled: Some(true),
                    ctrl_edge_switch_enabled: Some(false),
                    allow_plain_slide_to_internal: Some(false),
                    unify_cross_os_modifiers: Some(true),
                    auto_switch_input_provider_on_hotplug: Some(true),
                    internal_height_percent: Some(100),
                })
            })
        }

        fn handle_switch(&self, req: RemoteSwitchRequest) -> BoxFuture<'_, Result<ActionAck>> {
            Box::pin(async move {
                Ok(ActionAck {
                    ok: true,
                    status: "writing".to_string(),
                    message: format!("switching to {}", req.target_input),
                })
            })
        }

        fn handle_focus_input(&self, _req: FocusInputRequest) -> BoxFuture<'_, Result<ActionAck>> {
            Box::pin(async {
                Ok(ActionAck {
                    ok: true,
                    status: "focused".to_string(),
                    message: "ok".to_string(),
                })
            })
        }

        fn handle_brightness(&self, req: BrightnessRequest) -> BoxFuture<'_, Result<ActionAck>> {
            Box::pin(async move {
                Ok(ActionAck {
                    ok: true,
                    status: "applied".to_string(),
                    message: format!("brightness={}", req.brightness),
                })
            })
        }

        fn handle_volume(&self, req: VolumeRequest) -> BoxFuture<'_, Result<ActionAck>> {
            Box::pin(async move {
                Ok(ActionAck {
                    ok: true,
                    status: "applied".to_string(),
                    message: format!("vol={:?},muted={:?}", req.volume, req.muted),
                })
            })
        }

        fn handle_color_preset(&self, req: ColorPresetRequest) -> BoxFuture<'_, Result<ActionAck>> {
            Box::pin(async move {
                Ok(ActionAck {
                    ok: true,
                    status: "applied".to_string(),
                    message: format!("preset={}", req.preset),
                })
            })
        }

        fn handle_display_mode(&self, req: DisplayModeRequest) -> BoxFuture<'_, Result<ActionAck>> {
            Box::pin(async move {
                Ok(ActionAck {
                    ok: true,
                    status: "applied".to_string(),
                    message: format!("{}x{}@{}", req.width, req.height, req.hz),
                })
            })
        }

        fn handle_receive_file(
            &self,
            rel_path: PathBuf,
            data: Bytes,
        ) -> BoxFuture<'_, Result<ActionAck>> {
            Box::pin(async move {
                Ok(ActionAck {
                    ok: true,
                    status: "saved".to_string(),
                    message: format!("{}:{}", rel_path.to_string_lossy(), data.len()),
                })
            })
        }

        fn handle_receive_clipboard_png(&self, png_data: Bytes) -> BoxFuture<'_, Result<ActionAck>> {
            Box::pin(async move {
                Ok(ActionAck {
                    ok: true,
                    status: "synced".to_string(),
                    message: format!("png:{}", png_data.len()),
                })
            })
        }

        fn handle_display_sleep(&self) -> BoxFuture<'_, Result<ActionAck>> {
            Box::pin(async {
                Ok(ActionAck {
                    ok: true,
                    status: "sleeping".to_string(),
                    message: "ok".to_string(),
                })
            })
        }

        fn handle_wake(&self) -> BoxFuture<'_, Result<ActionAck>> {
            Box::pin(async {
                Ok(ActionAck {
                    ok: true,
                    status: "awake".to_string(),
                    message: "ok".to_string(),
                })
            })
        }

        fn handle_pair(&self, req: PairPeerRequest) -> BoxFuture<'_, Result<PairPeerResponse>> {
            Box::pin(async move {
                Ok(PairPeerResponse {
                    host_id: format!(
                        "ack-{}-{}-{}",
                        req.host_id,
                        req.input_provider_host_id.unwrap_or_default(),
                        req.allow_plain_slide_to_internal.unwrap_or(false)
                    ),
                    tls_fingerprint: "v2:sha256:serverfp".to_string(),
                    mac_address: None,
                })
            })
        }
    }

    #[tokio::test]
    async fn enforces_bearer_auth_and_rejects_tailscale_outbound() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = build_control_router(Arc::new(MockHandler));
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = LanControlClient::new().unwrap();
        assert!(client.status("100.64.0.42", port, "secret-lan-token").await.is_err());
        assert!(client.status("127.0.0.1", port, "wrong-token").await.is_err());

        let st = client.status("127.0.0.1", port, "secret-lan-token").await.unwrap();
        assert_eq!(st.host_id, "macbook");
        assert_eq!(st.observed_monitor_input, Some(17));
        assert_eq!(st.current_brightness, Some(85));

        let ack = client
            .request_switch(
                "127.0.0.1",
                port,
                "secret-lan-token",
                &RemoteSwitchRequest {
                    request_key: "k-1".to_string(),
                    source: SwitchSource::CtrlEdge,
                    from_host: Some("win-desktop".to_string()),
                    to_host: "macbook".to_string(),
                    target_input: 21,
                    sync_input_focus: true,
                },
            )
            .await
            .unwrap();
        assert_eq!(ack.status, "writing");

        let pair_ack = client
            .pair(
                "127.0.0.1",
                port,
                "secret-lan-token",
                &PairPeerRequest {
                    host_id: "macbook".to_string(),
                    lan_ip: "192.168.1.10".to_string(),
                    tls_fingerprint: "v2:sha256:1234".to_string(),
                    mac_address: None,
                    shared_settings_rev: Some(42),
                    input_provider_host_id: Some("macbook".to_string()),
                    primary_ddc_controller_id: Some("macbook".to_string()),
                    deskflow_enabled: Some(true),
                    ctrl_edge_switch_enabled: Some(true),
                    allow_plain_slide_to_internal: Some(false),
                    unify_cross_os_modifiers: Some(true),
                    auto_switch_input_provider_on_hotplug: Some(true),
                    internal_height_percent: Some(100),
                },
            )
            .await
            .unwrap();
        assert_eq!(pair_ack.host_id, "ack-macbook-macbook-false");

        // Verify SHA-256 verified file transfer & path traversal rejection
        assert!(sanitize_transfer_rel_path("../etc/passwd").is_err());
        assert!(sanitize_transfer_rel_path("/abs/path.txt").is_err());
        assert!(sanitize_transfer_rel_path("C:\\evil.exe").is_err());
        let xfer_ack = client
            .transfer_file(
                "127.0.0.1",
                port,
                "secret-lan-token",
                "设计稿/截图 01.png",
                b"hello-lan-bytes".to_vec(),
            )
            .await
            .unwrap();
        assert_eq!(xfer_ack.status, "saved");
        assert!(xfer_ack.message.contains("15"));
    }
}
