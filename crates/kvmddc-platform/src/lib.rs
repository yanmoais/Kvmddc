use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayModeOption {
    pub mode_id: i32,
    pub width: u32,
    pub height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub hz: u32,
    pub hidpi: bool,
    pub current: bool,
    pub is_builtin: bool,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectedMonitorProbe {
    pub connected: bool,
    pub model_name: Option<String>,
    /// VCP 0x60 input code inferred from physical link: 21 (USB-C), 17 (HDMI), 15 (DP)
    pub detected_input: Option<u8>,
    pub port_label: Option<String>,
    pub active_width: Option<u32>,
    pub active_height: Option<u32>,
    pub active_hz: Option<u32>,
    pub usb_c_pd_watts: Option<u16>,
}

impl ConnectedMonitorProbe {
    pub fn mode_summary(&self) -> Option<String> {
        match (self.active_width, self.active_height, self.active_hz) {
            (Some(w), Some(h), Some(hz)) if w > 0 && h > 0 && hz > 0 => {
                Some(format!("{w}×{h} · {hz}Hz"))
            }
            (Some(w), Some(h), _) if w > 0 && h > 0 => Some(format!("{w}×{h}")),
            _ => None,
        }
    }
}

pub fn default_port_label_for_input(input: u8) -> &'static str {
    match input {
        21 => "USB-C",
        17 => "HDMI 2.1",
        15 => "DP 1.4",
        _ => "视频信号",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointerSample {
    pub x: i32,
    pub y: i32,
    pub screen_left: i32,
    pub screen_top: i32,
    pub screen_right: i32,
    pub screen_bottom: i32,
    pub ctrl_down: bool,
}

impl PointerSample {
    pub fn near_edge(&self, margin_px: i32) -> Option<EdgeSide> {
        let width = self.screen_right - self.screen_left;
        if width < 200 {
            return None;
        }
        if self.x <= self.screen_left + margin_px {
            Some(EdgeSide::Left)
        } else if self.x >= self.screen_right - 1 - margin_px {
            Some(EdgeSide::Right)
        } else {
            None
        }
    }
}

/// Builds 6-byte DDC/CI `Set VCP Feature` I2C payload (`[0x84, 0x03, vcp, sh, sl, chk]`).
pub fn build_ddc_set_vcp_packet(vcp_code: u8, value: u16) -> [u8; 6] {
    let sh = (value >> 8) as u8;
    let sl = (value & 0xFF) as u8;
    let b0 = 0x84u8;
    let b1 = 0x03u8;
    let chk = 0x6Eu8 ^ 0x51u8 ^ b0 ^ b1 ^ vcp_code ^ sh ^ sl;
    [b0, b1, vcp_code, sh, sl, chk]
}

/// Builds 4-byte DDC/CI `Get VCP Feature` I2C request (`[0x82, 0x01, vcp, chk]`).
pub fn build_ddc_get_vcp_packet(vcp_code: u8) -> [u8; 4] {
    let b0 = 0x82u8;
    let b1 = 0x01u8;
    let chk = 0x6Eu8 ^ 0x51u8 ^ b0 ^ b1 ^ vcp_code;
    [b0, b1, vcp_code, chk]
}

/// Parses a 12-byte (or 11-byte) DDC/CI `Get VCP Feature Reply` into `(current_value, max_value)`.
pub fn parse_ddc_vcp_reply(reply: &[u8], expected_vcp: u8) -> Result<(u16, u16)> {
    ensure!(reply.len() >= 11, "DDC 应答长度不足: {}", reply.len());

    // Layout A (Apple DCPAVServiceProxy 12-byte buffer starting with 0x6E source address):
    // [0]=0x6E, [1]=0x88, [2]=0x02, [3]=result(0), [4]=vcp, [5]=type, [6..8]=max, [8..10]=cur, [10]=chk
    if reply[0] == 0x6E && (reply[1] & 0xF0) == 0x80 {
        ensure!(reply[2] == 0x02, "非 VCP 应答操作码: 0x{:02X}", reply[2]);
        ensure!(reply[3] == 0x00, "显示器返回不支持该寄存器 (result=0x{:02X})", reply[3]);
        ensure!(
            reply[4] == expected_vcp,
            "应答寄存器 (0x{:02X}) 与请求 (0x{:02X}) 不匹配",
            reply[4],
            expected_vcp
        );
        let mut calc = 0x50u8;
        for &b in &reply[0..10] {
            calc ^= b;
        }
        ensure!(
            calc == reply[10],
            "DDC 应答校验和错误 (expected 0x{:02X}, got 0x{:02X})",
            calc,
            reply[10]
        );
        let max_val = u16::from_be_bytes([reply[6], reply[7]]);
        let cur_val = u16::from_be_bytes([reply[8], reply[9]]);
        return Ok((cur_val, max_val));
    }

    // Layout B (without leading 0x6E byte):
    // [0]=0x88, [1]=0x02, [2]=result(0), [3]=vcp, [4]=type, [5..7]=max, [7..9]=cur, [9]=chk
    if (reply[0] & 0xF0) == 0x80 && reply[1] == 0x02 {
        ensure!(reply[2] == 0x00, "显示器返回不支持该寄存器 (result=0x{:02X})", reply[2]);
        ensure!(
            reply[3] == expected_vcp,
            "应答寄存器 (0x{:02X}) 与请求 (0x{:02X}) 不匹配",
            reply[3],
            expected_vcp
        );
        let mut calc = 0x50u8 ^ 0x6Eu8;
        for &b in &reply[0..9] {
            calc ^= b;
        }
        ensure!(
            calc == reply[9],
            "DDC 应答校验和错误 (expected 0x{:02X}, got 0x{:02X})",
            calc,
            reply[9]
        );
        let max_val = u16::from_be_bytes([reply[5], reply[6]]);
        let cur_val = u16::from_be_bytes([reply[7], reply[8]]);
        return Ok((cur_val, max_val));
    }

    bail!("无效 DDC/CI 应答帧头: {:02X?}", &reply[..reply.len().min(6)])
}

pub fn read_vcp(vcp_code: u8) -> Result<(u16, u16)> {
    #[cfg(target_os = "macos")]
    {
        macos::read_vcp(vcp_code)
    }
    #[cfg(target_os = "windows")]
    {
        windows::read_vcp(vcp_code)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = vcp_code;
        bail!("当前操作系统不支持 DDC 读写")
    }
}

pub fn write_vcp(vcp_code: u8, value: u16) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::write_vcp(vcp_code, value)
    }
    #[cfg(target_os = "windows")]
    {
        windows::write_vcp(vcp_code, value)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (vcp_code, value);
        bail!("当前操作系统不支持 DDC 读写")
    }
}

pub fn read_input_source() -> Result<u8> {
    let (cur, _max) = read_vcp(0x60)?;
    let low = (cur & 0xFF) as u8;
    ensure!(low != 0, "DDC 0x60 回读到非法值 0");
    Ok(low)
}

pub fn write_input_source(input: u8) -> Result<()> {
    ensure!(input >= 1, "输入源编码必须 >= 1");
    write_vcp(0x60, u16::from(input))
}

pub fn read_brightness() -> Result<u8> {
    let (cur, _max) = read_vcp(0x10)?;
    ensure!(cur <= 100, "DDC 0x10 亮度返回值超出范围: {cur}");
    Ok(cur as u8)
}

pub fn write_brightness(brightness: u8) -> Result<()> {
    ensure!(brightness <= 100, "亮度必须在 0..=100 之间");
    write_vcp(0x10, u16::from(brightness))
}

pub fn read_volume() -> Result<u8> {
    #[cfg(target_os = "macos")]
    {
        if let Some(v) = macos::read_system_volume() {
            return Ok(v);
        }
    }
    let (cur, _max) = read_vcp(0x62)?;
    Ok((cur.min(100)) as u8)
}

pub fn write_volume(volume: u8) -> Result<()> {
    ensure!(volume <= 100, "音量必须在 0..=100 之间");
    #[cfg(target_os = "macos")]
    {
        let sys_res = macos::write_system_volume(volume);
        let ddc_res = write_vcp(0x62, u16::from(volume));
        if sys_res.is_ok() || ddc_res.is_ok() {
            return Ok(());
        }
        return ddc_res.or(sys_res);
    }
    #[cfg(not(target_os = "macos"))]
    {
        write_vcp(0x62, u16::from(volume))
    }
}

/// MCCS VCP 0x8D: 1 = Mute, 2 = Unmute (verified on Philips 27E2N6900RW).
pub fn write_audio_mute(muted: bool) -> Result<()> {
    let code = if muted { 1u16 } else { 2u16 };
    #[cfg(target_os = "macos")]
    {
        let sys_res = macos::write_system_audio_mute(muted);
        let ddc_res = write_vcp(0x8D, code);
        if sys_res.is_ok() || ddc_res.is_ok() {
            return Ok(());
        }
        return ddc_res.or(sys_res);
    }
    #[cfg(not(target_os = "macos"))]
    {
        write_vcp(0x8D, code)
    }
}

pub fn kill_other_kvmddc_instances() {
    #[cfg(target_os = "macos")]
    {
        macos::kill_other_kvmddc_instances();
    }
    #[cfg(target_os = "windows")]
    {
        windows::kill_other_kvmddc_instances();
    }
}

pub fn read_color_preset() -> Result<u8> {
    let (cur, _max) = read_vcp(0x14)?;
    Ok((cur & 0xFF) as u8)
}

pub fn write_color_preset(preset: u8) -> Result<()> {
    ensure!(preset >= 1, "色温预设编码非法");
    write_vcp(0x14, u16::from(preset))
}

pub fn sleep_displays_keep_awake() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::sleep_displays_keep_awake()
    }
    #[cfg(target_os = "windows")]
    {
        windows::sleep_displays_keep_awake()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Ok(())
    }
}

pub fn wake_displays_and_wait() -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        macos::wake_displays_and_wait()
    }
    #[cfg(target_os = "windows")]
    {
        windows::wake_displays_and_wait()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Ok(true)
    }
}

pub fn heal_display_profile(
    preferred_width: u32,
    preferred_height: u32,
    preferred_hz: u32,
) -> Result<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        macos::heal_display_profile(preferred_width, preferred_height, preferred_hz)
    }
    #[cfg(target_os = "windows")]
    {
        windows::heal_display_profile(preferred_width, preferred_height, preferred_hz)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (preferred_width, preferred_height, preferred_hz);
        Ok(None)
    }
}

pub fn list_display_modes(prefer_external: bool) -> Vec<DisplayModeOption> {
    #[cfg(target_os = "macos")]
    {
        macos::list_display_modes(prefer_external)
    }
    #[cfg(target_os = "windows")]
    {
        windows::list_display_modes(prefer_external)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = prefer_external;
        Vec::new()
    }
}

pub fn apply_display_mode(
    mode_id: i32,
    width: u32,
    height: u32,
    hz: u32,
    hidpi: bool,
    prefer_external: bool,
) -> Result<DisplayModeOption> {
    #[cfg(target_os = "macos")]
    {
        macos::apply_display_mode(mode_id, width, height, hz, hidpi, prefer_external)
    }
    #[cfg(target_os = "windows")]
    {
        windows::apply_display_mode(mode_id, width, height, hz, hidpi, prefer_external)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (mode_id, width, height, hz, hidpi, prefer_external);
        bail!("当前操作系统不支持切换显示分辨率")
    }
}

pub fn apply_software_gamma(
    brightness_pct: u8,
    color_preset: u8,
    prefer_external: bool,
) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::apply_software_gamma(brightness_pct, color_preset, prefer_external)
    }
    #[cfg(target_os = "windows")]
    {
        windows::apply_software_gamma(brightness_pct, color_preset, prefer_external)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (brightness_pct, color_preset, prefer_external);
        Ok(())
    }
}

pub fn sample_pointer_and_ctrl() -> Option<PointerSample> {
    #[cfg(target_os = "macos")]
    {
        macos::sample_pointer_and_ctrl()
    }
    #[cfg(target_os = "windows")]
    {
        windows::sample_pointer_and_ctrl()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

pub fn is_outer_desktop_edge(x: i32, y: i32, side: EdgeSide) -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_outer_desktop_edge(x, y, side)
    }
    #[cfg(target_os = "windows")]
    {
        windows::is_outer_desktop_edge(x, y, side)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (x, y, side);
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAdjustHotkey {
    BrightnessUp,
    BrightnessDown,
    VolumeUp,
    VolumeDown,
    ToggleMute,
}

static LAST_HOTKEY_MASK: AtomicU8 = AtomicU8::new(0);
static LAST_MEDIA_MASK: AtomicU8 = AtomicU8::new(0);

/// Edge-triggered poll for `Ctrl + Option/Alt + 1/2/3`.
pub fn poll_user_hotkey_slot() -> Option<u8> {
    let cur_mask = raw_hotkey_slot_mask();
    let prev = LAST_HOTKEY_MASK.swap(cur_mask, Ordering::Relaxed);
    let newly_pressed = cur_mask & !prev;
    if (newly_pressed & 0b001) != 0 {
        Some(1)
    } else if (newly_pressed & 0b010) != 0 {
        Some(2)
    } else if (newly_pressed & 0b100) != 0 {
        Some(3)
    } else {
        None
    }
}

/// Edge-triggered poll for `Ctrl + Option/Alt + Up/Down/Right/Left/M`.
pub fn poll_media_adjust_hotkey() -> Option<MediaAdjustHotkey> {
    let cur = raw_media_adjust_mask();
    let prev = LAST_MEDIA_MASK.swap(cur, Ordering::Relaxed);
    let newly = cur & !prev;
    if (newly & 0b00001) != 0 {
        Some(MediaAdjustHotkey::BrightnessUp)
    } else if (newly & 0b00010) != 0 {
        Some(MediaAdjustHotkey::BrightnessDown)
    } else if (newly & 0b00100) != 0 {
        Some(MediaAdjustHotkey::VolumeUp)
    } else if (newly & 0b01000) != 0 {
        Some(MediaAdjustHotkey::VolumeDown)
    } else if (newly & 0b10000) != 0 {
        Some(MediaAdjustHotkey::ToggleMute)
    } else {
        None
    }
}

fn raw_hotkey_slot_mask() -> u8 {
    #[cfg(target_os = "macos")]
    {
        macos::raw_hotkey_slot_mask()
    }
    #[cfg(target_os = "windows")]
    {
        windows::raw_hotkey_slot_mask()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        0
    }
}

fn raw_media_adjust_mask() -> u8 {
    #[cfg(target_os = "macos")]
    {
        macos::raw_media_adjust_mask()
    }
    #[cfg(target_os = "windows")]
    {
        windows::raw_media_adjust_mask()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        0
    }
}

pub fn trigger_internal_deskflow_slot(slot: u8) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::trigger_internal_deskflow_slot(slot)
    }
    #[cfg(target_os = "windows")]
    {
        windows::trigger_internal_deskflow_slot(slot)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = slot;
        Ok(())
    }
}

pub fn is_edge_locked() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_edge_locked()
    }
    #[cfg(target_os = "windows")]
    {
        windows::is_edge_locked()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

pub fn kill_orphan_deskflow_processes() {
    #[cfg(target_os = "macos")]
    {
        macos::kill_orphan_deskflow_processes();
    }
    #[cfg(target_os = "windows")]
    {
        windows::kill_orphan_deskflow_processes();
    }
}

pub fn is_secure_input_enabled() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_secure_input_enabled()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn probe_connected_monitor() -> ConnectedMonitorProbe {
    #[cfg(target_os = "macos")]
    {
        macos::probe_connected_monitor()
    }
    #[cfg(target_os = "windows")]
    {
        windows::probe_connected_monitor()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        ConnectedMonitorProbe::default()
    }
}

pub fn is_accessibility_trusted() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_accessibility_trusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn prompt_and_open_accessibility_settings() -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        macos::prompt_and_open_accessibility_settings()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(true)
    }
}

pub fn has_external_display() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::has_external_display()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn is_clamshell_closed() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_clamshell_closed()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn gather_external_windows_to_builtin() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::gather_external_windows_to_builtin()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

pub fn read_clipboard_png() -> Option<Vec<u8>> {
    #[cfg(target_os = "macos")]
    {
        macos::read_clipboard_png()
    }
    #[cfg(target_os = "windows")]
    {
        windows::read_clipboard_png()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

pub fn write_clipboard_png(png_bytes: &[u8]) -> Result<()> {
    ensure!(!png_bytes.is_empty(), "剪贴板图像为空");
    #[cfg(target_os = "macos")]
    {
        macos::write_clipboard_png(png_bytes)
    }
    #[cfg(target_os = "windows")]
    {
        windows::write_clipboard_png(png_bytes)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Ok(())
    }
}

/// Converts PNG image bytes into uncompressed 32-bit BGRA bottom-up Win32 `CF_DIB` (`BITMAPINFOHEADER` + pixels).
pub fn png_to_cf_dib(png_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf)?;
    let w = info.width as usize;
    let h = info.height as usize;
    ensure!(w > 0 && h > 0 && w <= 16384 && h <= 16384, "图像尺寸超出限制");

    let mut dib = Vec::with_capacity(40 + w * h * 4);
    dib.extend_from_slice(&40u32.to_le_bytes()); // biSize
    dib.extend_from_slice(&(w as i32).to_le_bytes()); // biWidth
    dib.extend_from_slice(&(h as i32).to_le_bytes()); // biHeight (positive = bottom-up)
    dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    dib.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    dib.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    dib.extend_from_slice(&((w * h * 4) as u32).to_le_bytes()); // biSizeImage
    dib.extend_from_slice(&2835i32.to_le_bytes()); // biXPelsPerMeter
    dib.extend_from_slice(&2835i32.to_le_bytes()); // biYPelsPerMeter
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    let src = &buf[..info.buffer_size()];
    for row in (0..h).rev() {
        for col in 0..w {
            let (r, g, b, a) = match info.color_type {
                png::ColorType::Rgba => {
                    let o = (row * w + col) * 4;
                    (src[o], src[o + 1], src[o + 2], src[o + 3])
                }
                png::ColorType::Rgb => {
                    let o = (row * w + col) * 3;
                    (src[o], src[o + 1], src[o + 2], 255)
                }
                png::ColorType::GrayscaleAlpha => {
                    let o = (row * w + col) * 2;
                    (src[o], src[o], src[o], src[o + 1])
                }
                png::ColorType::Grayscale => {
                    let v = src[row * w + col];
                    (v, v, v, 255)
                }
                _ => bail!("不支持的 PNG 像素格式"),
            };
            dib.extend_from_slice(&[b, g, r, a]);
        }
    }
    Ok(dib)
}

/// Converts uncompressed 24-bit or 32-bit Win32 `CF_DIB` (`BITMAPINFOHEADER` + pixels) into PNG bytes.
pub fn cf_dib_to_png(dib: &[u8]) -> Result<Vec<u8>> {
    ensure!(dib.len() > 40, "DIB 数据过短");
    let hdr_size = u32::from_le_bytes(dib[0..4].try_into().unwrap()) as usize;
    ensure!(hdr_size >= 40 && dib.len() > hdr_size, "无效 BITMAPINFOHEADER");
    let w_i32 = i32::from_le_bytes(dib[4..8].try_into().unwrap());
    let h_i32 = i32::from_le_bytes(dib[8..12].try_into().unwrap());
    let bpp = u16::from_le_bytes(dib[14..16].try_into().unwrap());
    let comp = u32::from_le_bytes(dib[16..20].try_into().unwrap());
    ensure!(w_i32 > 0 && h_i32 != 0, "无效 DIB 宽高");
    ensure!(bpp == 32 || bpp == 24, "仅支持 24/32 位 DIB");
    // BI_RGB = 0, BI_BITFIELDS = 3
    let pixel_offset = if comp == 3 && hdr_size == 40 {
        ensure!(dib.len() > 52, "BI_BITFIELDS 掩码长度不足");
        52usize
    } else {
        ensure!(comp == 0 || comp == 3, "不支持压缩 DIB ({comp})");
        hdr_size
    };

    let w = w_i32 as usize;
    let top_down = h_i32 < 0;
    let h = h_i32.unsigned_abs() as usize;
    let row_stride = if bpp == 32 {
        w * 4
    } else {
        (w * 3 + 3) & !3
    };
    ensure!(
        dib.len() >= pixel_offset + row_stride * h,
        "DIB 像素区长度不足"
    );

    let pixels = &dib[pixel_offset..];
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        let src_row = if top_down { y } else { h - 1 - y };
        let row_slice = &pixels[src_row * row_stride..(src_row + 1) * row_stride];
        for x in 0..w {
            let dst = (y * w + x) * 4;
            if bpp == 32 {
                let s = x * 4;
                rgba[dst] = row_slice[s + 2];
                rgba[dst + 1] = row_slice[s + 1];
                rgba[dst + 2] = row_slice[s];
                // Many Win32 GDI apps (e.g. BitBlt PrintScreen) leave alpha=0 in 32-bit BI_RGB DIBs
                rgba[dst + 3] = 255;
            } else {
                let s = x * 3;
                rgba[dst] = row_slice[s + 2];
                rgba[dst + 1] = row_slice[s + 1];
                rgba[dst + 2] = row_slice[s];
                rgba[dst + 3] = 255;
            }
        }
    }

    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header()?;
        writer.write_image_data(&rgba)?;
    }
    Ok(out)
}

/// Stateful detector that arms after a short button-release grace window and triggers
/// immediately on any real keyboard keypress (including modifier keys) or mouse button click,
/// while ignoring 1px mouse sensor drift and background keep-awake assertions (e.g. UURemote).
pub struct StandbyWakeDetector {
    armed_at: Instant,
    #[allow(dead_code)]
    baseline_ready: bool,
    #[allow(dead_code)]
    prev_down: [bool; 256],
}

impl Default for StandbyWakeDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl StandbyWakeDetector {
    pub fn new() -> Self {
        Self {
            armed_at: Instant::now(),
            baseline_ready: false,
            prev_down: [false; 256],
        }
    }

    pub fn reset(&mut self) {
        self.armed_at = Instant::now();
        self.baseline_ready = false;
        self.prev_down = [false; 256];
    }

    pub fn poll_triggered(&mut self) -> bool {
        #[cfg(target_os = "macos")]
        {
            let elapsed = self.armed_at.elapsed();
            if elapsed < Duration::from_millis(520) {
                return false;
            }
            macos::has_recent_wake_input(elapsed.as_secs_f64())
        }
        #[cfg(target_os = "windows")]
        {
            if self.armed_at.elapsed() < Duration::from_millis(380) {
                return false;
            }
            let mut cur = [false; 256];
            windows::snapshot_pressed_vks(&mut cur);
            if !self.baseline_ready {
                self.prev_down = cur;
                self.baseline_ready = true;
                return false;
            }
            let mut triggered = false;
            for vk in 0..256 {
                if cur[vk] && !self.prev_down[vk] {
                    triggered = true;
                }
            }
            self.prev_down = cur;
            triggered
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddc_packet_checksums_and_reply_parser_are_exact() {
        let set_pkt = build_ddc_set_vcp_packet(0x60, 0x0015); // USB-C = 21
        assert_eq!(set_pkt[0..5], [0x84, 0x03, 0x60, 0x00, 0x15]);
        assert_eq!(set_pkt[5], 0x6E ^ 0x51 ^ 0x84 ^ 0x03 ^ 0x60 ^ 0x00 ^ 0x15);

        let get_pkt = build_ddc_get_vcp_packet(0x10);
        assert_eq!(get_pkt[0..3], [0x82, 0x01, 0x10]);
        assert_eq!(get_pkt[3], 0x6E ^ 0x51 ^ 0x82 ^ 0x01 ^ 0x10);

        let mut reply = [0x6Eu8, 0x88, 0x02, 0x00, 0x60, 0x00, 0x00, 0x15, 0x00, 0x11, 0x00, 0x00];
        let mut chk = 0x50u8;
        for &b in &reply[0..10] {
            chk ^= b;
        }
        reply[10] = chk;

        let (cur, max) = parse_ddc_vcp_reply(&reply, 0x60).unwrap();
        assert_eq!(cur, 17);
        assert_eq!(max, 21);

        // Tampered checksum must fail
        reply[10] ^= 0x01;
        assert!(parse_ddc_vcp_reply(&reply, 0x60).is_err());
    }

    #[test]
    fn pointer_edge_detection_respects_margin() {
        let sample = PointerSample {
            x: 2558,
            y: 600,
            screen_left: 0,
            screen_top: 0,
            screen_right: 2560,
            screen_bottom: 1440,
            ctrl_down: true,
        };
        assert_eq!(sample.near_edge(4), Some(EdgeSide::Right));
    }

    #[test]
    fn monitor_probe_formatting_and_live_smoke() {
        let p = ConnectedMonitorProbe {
            connected: true,
            model_name: Some("27E2N6900RW".into()),
            detected_input: Some(17),
            port_label: Some("HDMI 2.1".into()),
            active_width: Some(5120),
            active_height: Some(2880),
            active_hz: Some(120),
            usb_c_pd_watts: None,
        };
        assert_eq!(p.mode_summary().as_deref(), Some("5120×2880 · 120Hz"));
        assert_eq!(default_port_label_for_input(21), "USB-C");
        assert_eq!(default_port_label_for_input(17), "HDMI 2.1");
        assert_eq!(default_port_label_for_input(15), "DP 1.4");

        let live = probe_connected_monitor();
        println!("LIVE_PROBE={live:?}");
    }

    #[test]
    fn standby_wake_detector_respects_arming_grace() {
        let mut det = StandbyWakeDetector::new();
        // Immediate poll right after arming must never self-trigger from the sleep click itself
        assert!(!det.poll_triggered());
    }

    #[test]
    fn png_and_cf_dib_roundtrip_cleanly() {
        let mut raw_png = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut raw_png, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[
                255, 0, 0, 255, 0, 255, 0, 255, // row 0: red, green
                0, 0, 255, 255, 255, 255, 255, 255, // row 1: blue, white
            ])
            .unwrap();
        }
        let dib = png_to_cf_dib(&raw_png).unwrap();
        assert_eq!(dib.len(), 40 + 16);
        let back_png = cf_dib_to_png(&dib).unwrap();
        let dib2 = png_to_cf_dib(&back_png).unwrap();
        assert_eq!(dib, dib2);
    }
}
