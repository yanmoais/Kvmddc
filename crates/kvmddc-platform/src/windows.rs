use crate::{ConnectedMonitorProbe, DisplayModeOption, EdgeSide, PointerSample};
use anyhow::{bail, ensure, Result};
use std::ffi::c_void;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    Mutex, Once,
};
use std::thread;
use std::time::Duration;

type HANDLE = *mut c_void;
type HWND = *mut c_void;
type HMONITOR = *mut c_void;
type HDC = *mut c_void;
type BOOL = i32;
type DWORD = u32;
type WORD = u16;
type LONG = i32;
type UINT = u32;
type WPARAM = usize;
type LPARAM = isize;

const MONITOR_DEFAULTTONEAREST: DWORD = 2;
const ENUM_CURRENT_SETTINGS: DWORD = u32::MAX;
const ENUM_REGISTRY_SETTINGS: DWORD = 0xFFFF_FFFE;
const DM_BITSPERPEL: DWORD = 0x0004_0000;
const DM_PELSWIDTH: DWORD = 0x0008_0000;
const DM_PELSHEIGHT: DWORD = 0x0010_0000;
const DM_DISPLAYFREQUENCY: DWORD = 0x0040_0000;
const CDS_UPDATEREGISTRY: DWORD = 0x0000_0001;
const DISPLAY_DEVICE_ACTIVE: DWORD = 0x0000_0001;

const WM_SYSCOMMAND: UINT = 0x0112;
const SC_MONITORPOWER: WPARAM = 0xF170;
const HWND_BROADCAST: HWND = 0xFFFF as HWND;

const VK_CONTROL: i32 = 0x11;
const VK_MENU: i32 = 0x12; // Alt
const VK_LEFT: i32 = 0x25;
const VK_UP: i32 = 0x26;
const VK_RIGHT: i32 = 0x27;
const VK_DOWN: i32 = 0x28;
const VK_M: i32 = 0x4D;
const VK_F18: WORD = 0x81;
const VK_F19: WORD = 0x82;
const VK_F20: WORD = 0x83;

const CF_DIB: UINT = 8;
const GMEM_MOVEABLE: UINT = 0x0002;

const INPUT_MOUSE: DWORD = 0;
const INPUT_KEYBOARD: DWORD = 1;
const KEYEVENTF_KEYUP: DWORD = 0x0002;
const MOUSEEVENTF_MOVE: DWORD = 0x0001;

const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;
const QDC_ONLY_ACTIVE_PATHS: u32 = 0x0000_0002;
const QDC_DATABASE_CURRENT: u32 = 0x0000_0004;
const DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME: u32 = 1;
const DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME: u32 = 2;
const DISPLAYCONFIG_OUTPUT_TECHNOLOGY_HDMI: u32 = 5;
const DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EXTERNAL: u32 = 10;
const DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_USB_TUNNEL: u32 = 18;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LUID {
    low_part: u32,
    high_part: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DISPLAYCONFIG_RATIONAL {
    numerator: u32,
    denominator: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DISPLAYCONFIG_PATH_SOURCE_INFO {
    adapter_id: LUID,
    id: u32,
    mode_info_idx: u32,
    status_flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DISPLAYCONFIG_PATH_TARGET_INFO {
    adapter_id: LUID,
    id: u32,
    mode_info_idx: u32,
    output_technology: u32,
    rotation: u32,
    scaling: u32,
    refresh_rate: DISPLAYCONFIG_RATIONAL,
    scan_line_ordering: u32,
    target_available: i32,
    status_flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DISPLAYCONFIG_PATH_INFO {
    source_info: DISPLAYCONFIG_PATH_SOURCE_INFO,
    target_info: DISPLAYCONFIG_PATH_TARGET_INFO,
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DISPLAYCONFIG_MODE_INFO {
    info_type: u32,
    id: u32,
    adapter_id: LUID,
    payload: [u32; 12],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DISPLAYCONFIG_DEVICE_INFO_HEADER {
    r#type: u32,
    size: u32,
    adapter_id: LUID,
    id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DISPLAYCONFIG_SOURCE_DEVICE_NAME {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    view_gdi_device_name: [u16; 32],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DISPLAYCONFIG_TARGET_DEVICE_NAME {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    flags: u32,
    output_technology: u32,
    edid_manufacture_id: u16,
    edid_product_code_id: u16,
    connector_instance: u32,
    monitor_friendly_device_name: [u16; 64],
    monitor_device_path: [u16; 128],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct POINT {
    x: LONG,
    y: LONG,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RECT {
    left: LONG,
    top: LONG,
    right: LONG,
    bottom: LONG,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MONITORINFO {
    cb_size: DWORD,
    rc_monitor: RECT,
    rc_work: RECT,
    dw_flags: DWORD,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PHYSICAL_MONITOR {
    h_physical_monitor: HANDLE,
    sz_physical_monitor_description: [u16; 128],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DISPLAY_DEVICEW {
    cb: DWORD,
    device_name: [u16; 32],
    device_string: [u16; 128],
    state_flags: DWORD,
    device_id: [u16; 128],
    device_key: [u16; 128],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DEVMODEW {
    dm_device_name: [u16; 32],
    dm_spec_version: WORD,
    dm_driver_version: WORD,
    dm_size: WORD,
    dm_driver_extra: WORD,
    dm_fields: DWORD,
    dm_position_x: LONG,
    dm_position_y: LONG,
    dm_display_orientation: DWORD,
    dm_display_fixed_output: DWORD,
    dm_color: i16,
    dm_duplex: i16,
    dm_y_resolution: i16,
    dm_tt_option: i16,
    dm_collate: i16,
    dm_form_name: [u16; 32],
    dm_log_pixels: WORD,
    dm_bits_per_pel: DWORD,
    dm_pels_width: DWORD,
    dm_pels_height: DWORD,
    dm_display_flags: DWORD,
    dm_display_frequency: DWORD,
    dm_icm_method: DWORD,
    dm_icm_intent: DWORD,
    dm_media_type: DWORD,
    dm_dither_type: DWORD,
    dm_reserved1: DWORD,
    dm_reserved2: DWORD,
    dm_panning_width: DWORD,
    dm_panning_height: DWORD,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct KEYBDINPUT {
    w_vk: WORD,
    w_scan: WORD,
    dw_flags: DWORD,
    time: DWORD,
    dw_extra_info: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MOUSEINPUT {
    dx: LONG,
    dy: LONG,
    mouse_data: DWORD,
    dw_flags: DWORD,
    time: DWORD,
    dw_extra_info: usize,
}

#[repr(C)]
union INPUT_UNION {
    mi: MOUSEINPUT,
    ki: KEYBDINPUT,
}

#[repr(C)]
struct INPUT {
    r#type: DWORD,
    u: INPUT_UNION,
}

type MonitorEnumProc = unsafe extern "system" fn(HMONITOR, HDC, *mut RECT, LPARAM) -> BOOL;

#[link(name = "user32")]
extern "system" {
    fn SetProcessDpiAwarenessContext(value: isize) -> BOOL;
    fn EnumDisplayMonitors(
        hdc: HDC,
        lprc_clip: *const RECT,
        lpfn_enum: MonitorEnumProc,
        dw_data: LPARAM,
    ) -> BOOL;
    fn MonitorFromPoint(pt: POINT, dw_flags: DWORD) -> HMONITOR;
    fn MonitorFromWindow(hwnd: HWND, dw_flags: DWORD) -> HMONITOR;
    fn GetMonitorInfoW(hmonitor: HMONITOR, lpmi: *mut MONITORINFO) -> BOOL;
    fn EnumDisplayDevicesW(
        lp_device: *const u16,
        i_dev_num: DWORD,
        lp_display_device: *mut DISPLAY_DEVICEW,
        dw_flags: DWORD,
    ) -> BOOL;
    fn EnumDisplaySettingsExW(
        lpsz_device_name: *const u16,
        i_mode_num: DWORD,
        lp_dev_mode: *mut DEVMODEW,
        dw_flags: DWORD,
    ) -> BOOL;
    fn ChangeDisplaySettingsExW(
        lpsz_device_name: *const u16,
        lp_dev_mode: *mut DEVMODEW,
        hwnd: HWND,
        dwflags: DWORD,
        lparam: *mut c_void,
    ) -> LONG;
    fn GetCursorPos(lp_point: *mut POINT) -> BOOL;
    fn GetAsyncKeyState(v_key: i32) -> i16;
    fn SendInput(c_inputs: UINT, p_inputs: *const INPUT, cb_size: i32) -> UINT;
    fn PostMessageW(h_wnd: HWND, msg: UINT, w_param: WPARAM, l_param: LPARAM) -> BOOL;
    fn GetForegroundWindow() -> HWND;
    fn GetWindowRect(h_wnd: HWND, lp_rect: *mut RECT) -> BOOL;
    fn GetClassNameW(h_wnd: HWND, lp_class_name: *mut u16, n_max_count: i32) -> i32;
    fn GetSystemMetrics(n_index: i32) -> i32;
    fn GetDisplayConfigBufferSizes(
        flags: u32,
        num_path_array_elements: *mut u32,
        num_mode_info_array_elements: *mut u32,
    ) -> i32;
    fn QueryDisplayConfig(
        flags: u32,
        num_path_array_elements: *mut u32,
        path_array: *mut DISPLAYCONFIG_PATH_INFO,
        num_mode_info_array_elements: *mut u32,
        mode_info_array: *mut DISPLAYCONFIG_MODE_INFO,
        current_topology_id: *mut u32,
    ) -> i32;
    fn DisplayConfigGetDeviceInfo(request_packet: *mut DISPLAYCONFIG_DEVICE_INFO_HEADER) -> i32;
    fn OpenClipboard(h_wnd_new_owner: HWND) -> BOOL;
    fn CloseClipboard() -> BOOL;
    fn EmptyClipboard() -> BOOL;
    fn GetClipboardSequenceNumber() -> DWORD;
    fn IsClipboardFormatAvailable(format: UINT) -> BOOL;
    fn GetClipboardData(u_format: UINT) -> HANDLE;
    fn SetClipboardData(u_format: UINT, h_mem: HANDLE) -> HANDLE;
    fn RegisterClipboardFormatW(lpsz_format: *const u16) -> UINT;
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalAlloc(u_flags: UINT, dw_bytes: usize) -> HANDLE;
    fn GlobalLock(h_mem: HANDLE) -> *mut u8;
    fn GlobalUnlock(h_mem: HANDLE) -> BOOL;
    fn GlobalFree(h_mem: HANDLE) -> HANDLE;
    fn GlobalSize(h_mem: HANDLE) -> usize;
}

#[link(name = "dxva2")]
extern "system" {
    fn GetNumberOfPhysicalMonitorsFromHMONITOR(
        h_monitor: HMONITOR,
        pdw_number_of_physical_monitors: *mut DWORD,
    ) -> BOOL;
    fn GetPhysicalMonitorsFromHMONITOR(
        h_monitor: HMONITOR,
        dw_physical_monitor_array_size: DWORD,
        p_physical_monitor_array: *mut PHYSICAL_MONITOR,
    ) -> BOOL;
    fn DestroyPhysicalMonitors(
        dw_physical_monitor_array_size: DWORD,
        p_physical_monitor_array: *const PHYSICAL_MONITOR,
    ) -> BOOL;
    fn GetVCPFeatureAndVCPFeatureReply(
        h_monitor: HANDLE,
        b_vcp_code: u8,
        pvct: *mut u32,
        pdw_current_value: *mut DWORD,
        pdw_maximum_value: *mut DWORD,
    ) -> BOOL;
    fn SetVCPFeature(h_monitor: HANDLE, b_vcp_code: u8, dw_new_value: DWORD) -> BOOL;
}

static DPI_INIT: Once = Once::new();

fn ensure_dpi_aware() {
    DPI_INIT.call_once(|| unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    });
}

unsafe extern "system" fn collect_hmonitors(
    hmon: HMONITOR,
    _hdc: HDC,
    _rc: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let vec = &mut *(lparam as *mut Vec<HMONITOR>);
    vec.push(hmon);
    1
}

fn with_first_physical_monitor<T, F: FnMut(HANDLE) -> Result<T>>(mut f: F) -> Result<T> {
    let mut monitors: Vec<HMONITOR> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            collect_hmonitors,
            &mut monitors as *mut Vec<HMONITOR> as LPARAM,
        );
    }

    let mut last_err = None;
    for hmon in monitors {
        let mut count: DWORD = 0;
        let ok = unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut count) };
        if ok == 0 || count == 0 {
            continue;
        }
        let mut phys = vec![
            PHYSICAL_MONITOR {
                h_physical_monitor: std::ptr::null_mut(),
                sz_physical_monitor_description: [0u16; 128],
            };
            count as usize
        ];
        let got = unsafe { GetPhysicalMonitorsFromHMONITOR(hmon, count, phys.as_mut_ptr()) };
        if got == 0 {
            continue;
        }
        let res = f(phys[0].h_physical_monitor);
        unsafe {
            let _ = DestroyPhysicalMonitors(count, phys.as_ptr());
        }
        match res {
            Ok(v) => return Ok(v),
            Err(e) => last_err = Some(e),
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("未找到可用的物理显示器 DDC 句柄")))
}

pub fn read_vcp(vcp_code: u8) -> Result<(u16, u16)> {
    with_first_physical_monitor(|handle| {
        let mut vct: u32 = 0;
        let mut cur: DWORD = 0;
        let mut max: DWORD = 0;
        let ok = unsafe {
            GetVCPFeatureAndVCPFeatureReply(handle, vcp_code, &mut vct, &mut cur, &mut max)
        };
        ensure!(ok != 0, "dxva2 读取 VCP 0x{vcp_code:02X} 失败");
        Ok(((cur & 0xFFFF) as u16, (max & 0xFFFF) as u16))
    })
}

pub fn write_vcp(vcp_code: u8, value: u16) -> Result<()> {
    with_first_physical_monitor(|handle| {
        let ok = unsafe { SetVCPFeature(handle, vcp_code, DWORD::from(value)) };
        ensure!(ok != 0, "dxva2 写入 VCP 0x{vcp_code:02X}={value} 失败");
        Ok(())
    })
}

static WINDOWS_STANDBY_ACTIVE: AtomicBool = AtomicBool::new(false);
static WINDOWS_STANDBY_SEQ: AtomicU64 = AtomicU64::new(0);

pub fn sleep_displays_keep_awake() -> Result<()> {
    WINDOWS_STANDBY_ACTIVE.store(true, Ordering::SeqCst);
    let seq = WINDOWS_STANDBY_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    // Delay SC_MONITORPOWER by 550ms so the mouse button release from clicking "熄屏待机"
    // does not immediately cancel Windows monitor power-off.
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(550));
        if WINDOWS_STANDBY_ACTIVE.load(Ordering::SeqCst)
            && WINDOWS_STANDBY_SEQ.load(Ordering::SeqCst) == seq
        {
            unsafe {
                let _ = PostMessageW(HWND_BROADCAST, WM_SYSCOMMAND, SC_MONITORPOWER, 2);
            }
        }
    });
    Ok(())
}

pub fn wake_displays_and_wait() -> Result<bool> {
    WINDOWS_STANDBY_ACTIVE.store(false, Ordering::SeqCst);
    WINDOWS_STANDBY_SEQ.fetch_add(1, Ordering::SeqCst);
    unsafe {
        let _ = PostMessageW(HWND_BROADCAST, WM_SYSCOMMAND, SC_MONITORPOWER, -1);
    }
    let move_pos = INPUT {
        r#type: INPUT_MOUSE,
        u: INPUT_UNION {
            mi: MOUSEINPUT {
                dx: 1,
                dy: 0,
                mouse_data: 0,
                dw_flags: MOUSEEVENTF_MOVE,
                time: 0,
                dw_extra_info: 0,
            },
        },
    };
    let move_back = INPUT {
        r#type: INPUT_MOUSE,
        u: INPUT_UNION {
            mi: MOUSEINPUT {
                dx: -1,
                dy: 0,
                mouse_data: 0,
                dw_flags: MOUSEEVENTF_MOVE,
                time: 0,
                dw_extra_info: 0,
            },
        },
    };
    unsafe {
        let _ = SendInput(1, &move_pos, std::mem::size_of::<INPUT>() as i32);
        let _ = SendInput(1, &move_back, std::mem::size_of::<INPUT>() as i32);
    }
    Ok(true)
}

pub fn snapshot_pressed_vks(out: &mut [bool; 256]) {
    const MOUSE_BUTTONS: [usize; 5] = [0x01, 0x02, 0x04, 0x05, 0x06];
    unsafe {
        for &vk in &MOUSE_BUTTONS {
            out[vk] = ((GetAsyncKeyState(vk as i32) as u16) & 0x8000) != 0;
        }
        for vk in 0x08usize..=0xFEusize {
            out[vk] = ((GetAsyncKeyState(vk as i32) as u16) & 0x8000) != 0;
        }
    }
}

fn utf16_prefix_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Automatically restores `preferred_width x preferred_height @ preferred_hz` (e.g. `5120x2880@75Hz`)
/// when the monitor is in 5K mode (5K mode exists in EDID modes and active refresh < 240Hz)
/// yet Windows dropped to a lower fallback mode like `2560x1440@59Hz`.
pub fn heal_display_profile(
    preferred_width: u32,
    preferred_height: u32,
    preferred_hz: u32,
) -> Result<Option<String>> {
    let mut dev_idx = 0;
    loop {
        let mut dev: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
        dev.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as DWORD;
        let ok = unsafe { EnumDisplayDevicesW(std::ptr::null(), dev_idx, &mut dev, 0) };
        if ok == 0 {
            break;
        }
        dev_idx += 1;

        if (dev.state_flags & DISPLAY_DEVICE_ACTIVE) == 0 {
            continue;
        }
        let dev_str = utf16_prefix_to_string(&dev.device_string).to_ascii_lowercase();
        if dev_str.contains("gameviewer") || dev_str.contains("virtual") {
            continue;
        }

        let mut cur_mode: DEVMODEW = unsafe { std::mem::zeroed() };
        cur_mode.dm_size = std::mem::size_of::<DEVMODEW>() as WORD;
        if unsafe {
            EnumDisplaySettingsExW(
                dev.device_name.as_ptr(),
                ENUM_CURRENT_SETTINGS,
                &mut cur_mode,
                0,
            )
        } == 0
        {
            continue;
        }

        // Respect intentional 300Hz esports dual-mode (>= 240Hz)
        if cur_mode.dm_display_frequency >= 240 {
            continue;
        }
        // Never re-apply ChangeDisplaySettingsExW when live resolution already matches
        // preferred dimensions at a normal refresh rate (>= 59Hz), avoiding NVIDIA DSC blanking
        // caused by Win11 CCD vs legacy ENUM_REGISTRY_SETTINGS frequency mismatches.
        if cur_mode.dm_pels_width == preferred_width
            && cur_mode.dm_pels_height == preferred_height
            && cur_mode.dm_display_frequency >= 59
        {
            continue;
        }
        let mut reg_mode: DEVMODEW = unsafe { std::mem::zeroed() };
        reg_mode.dm_size = std::mem::size_of::<DEVMODEW>() as WORD;
        let has_reg = unsafe {
            EnumDisplaySettingsExW(
                dev.device_name.as_ptr(),
                ENUM_REGISTRY_SETTINGS,
                &mut reg_mode,
                0,
            )
        } != 0;
        let reg_matches = !has_reg
            || (reg_mode.dm_pels_width == preferred_width
                && reg_mode.dm_pels_height == preferred_height
                && reg_mode.dm_display_frequency == preferred_hz);

        if cur_mode.dm_pels_width == preferred_width
            && cur_mode.dm_pels_height == preferred_height
            && cur_mode.dm_display_frequency == preferred_hz
            && reg_matches
        {
            continue;
        }

        // Check if preferred mode is supported in current EDID
        let mut target_mode: Option<DEVMODEW> = None;
        let mut mode_idx = 0;
        loop {
            let mut m: DEVMODEW = unsafe { std::mem::zeroed() };
            m.dm_size = std::mem::size_of::<DEVMODEW>() as WORD;
            if unsafe { EnumDisplaySettingsExW(dev.device_name.as_ptr(), mode_idx, &mut m, 0) }
                == 0
            {
                break;
            }
            mode_idx += 1;
            if m.dm_pels_width == preferred_width
                && m.dm_pels_height == preferred_height
                && m.dm_display_frequency == preferred_hz
            {
                target_mode = Some(m);
                break;
            }
        }

        if let Some(mut best) = target_mode {
            if best.dm_bits_per_pel < 32 {
                best.dm_bits_per_pel = 32;
            }
            best.dm_fields = DM_BITSPERPEL | DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
            let mut rc = unsafe {
                ChangeDisplaySettingsExW(
                    dev.device_name.as_ptr(),
                    &mut best,
                    std::ptr::null_mut(),
                    CDS_UPDATEREGISTRY,
                    std::ptr::null_mut(),
                )
            };
            if rc != 0 {
                rc = unsafe {
                    ChangeDisplaySettingsExW(
                        dev.device_name.as_ptr(),
                        &mut best,
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null_mut(),
                    )
                };
            }
            if rc == 0 {
                return Ok(Some(format!(
                    "已自动恢复最佳显示模式 {}×{}@{}Hz",
                    preferred_width, preferred_height, preferred_hz
                )));
            }
        }
    }
    Ok(None)
}

pub fn sample_pointer_and_ctrl() -> Option<PointerSample> {
    ensure_dpi_aware();
    unsafe {
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt) == 0 {
            return None;
        }
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        if hmon.is_null() {
            return None;
        }
        let mut mi = MONITORINFO {
            cb_size: std::mem::size_of::<MONITORINFO>() as DWORD,
            rc_monitor: RECT::default(),
            rc_work: RECT::default(),
            dw_flags: 0,
        };
        if GetMonitorInfoW(hmon, &mut mi) == 0 {
            return None;
        }
        let ctrl_down = ((GetAsyncKeyState(VK_CONTROL) as u16) & 0x8000) != 0;
        Some(PointerSample {
            x: pt.x,
            y: pt.y,
            screen_left: mi.rc_monitor.left,
            screen_top: mi.rc_monitor.top,
            screen_right: mi.rc_monitor.right,
            screen_bottom: mi.rc_monitor.bottom,
            ctrl_down,
        })
    }
}

pub fn is_outer_desktop_edge(x: i32, y: i32, side: EdgeSide) -> bool {
    ensure_dpi_aware();
    let probe_x = match side {
        EdgeSide::Left => x - 16,
        EdgeSide::Right => x + 16,
    };
    unsafe {
        let hmon = MonitorFromPoint(POINT { x: probe_x, y }, 0); // MONITOR_DEFAULTTONULL = 0
        hmon.is_null()
    }
}

pub fn raw_hotkey_slot_mask() -> u8 {
    unsafe {
        let ctrl = ((GetAsyncKeyState(VK_CONTROL) as u16) & 0x8000) != 0;
        let alt = ((GetAsyncKeyState(VK_MENU) as u16) & 0x8000) != 0;
        if !ctrl || !alt {
            return 0;
        }
        let mut mask = 0u8;
        if ((GetAsyncKeyState(0x31) as u16) & 0x8000) != 0 {
            mask |= 0b001;
        }
        if ((GetAsyncKeyState(0x32) as u16) & 0x8000) != 0 {
            mask |= 0b010;
        }
        if ((GetAsyncKeyState(0x33) as u16) & 0x8000) != 0 {
            mask |= 0b100;
        }
        mask
    }
}

pub fn raw_media_adjust_mask() -> u8 {
    unsafe {
        let ctrl = ((GetAsyncKeyState(VK_CONTROL) as u16) & 0x8000) != 0;
        let alt = ((GetAsyncKeyState(VK_MENU) as u16) & 0x8000) != 0;
        if !ctrl || !alt {
            return 0;
        }
        let mut mask = 0u8;
        if ((GetAsyncKeyState(VK_UP) as u16) & 0x8000) != 0 {
            mask |= 0b00001;
        }
        if ((GetAsyncKeyState(VK_DOWN) as u16) & 0x8000) != 0 {
            mask |= 0b00010;
        }
        if ((GetAsyncKeyState(VK_RIGHT) as u16) & 0x8000) != 0 {
            mask |= 0b00100;
        }
        if ((GetAsyncKeyState(VK_LEFT) as u16) & 0x8000) != 0 {
            mask |= 0b01000;
        }
        if ((GetAsyncKeyState(VK_M) as u16) & 0x8000) != 0 {
            mask |= 0b10000;
        }
        mask
    }
}

static LAST_CLIPBOARD_READ_SEQ: AtomicU32 = AtomicU32::new(0);

fn png_clipboard_format() -> UINT {
    let name: [u16; 4] = [b'P' as u16, b'N' as u16, b'G' as u16, 0];
    unsafe { RegisterClipboardFormatW(name.as_ptr()) }
}

pub fn read_clipboard_png() -> Option<Vec<u8>> {
    unsafe {
        let seq = GetClipboardSequenceNumber();
        if seq != 0 && seq == LAST_CLIPBOARD_READ_SEQ.load(Ordering::Relaxed) {
            return None;
        }
        let png_fmt = png_clipboard_format();
        let has_png = png_fmt != 0 && IsClipboardFormatAvailable(png_fmt) != 0;
        let has_dib = IsClipboardFormatAvailable(CF_DIB) != 0;
        if !has_png && !has_dib {
            if seq != 0 {
                LAST_CLIPBOARD_READ_SEQ.store(seq, Ordering::Relaxed);
            }
            return None;
        }
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }

        let mut out: Option<Vec<u8>> = None;
        if has_png {
            let h = GetClipboardData(png_fmt);
            if !h.is_null() {
                let sz = GlobalSize(h);
                if sz > 8 && sz <= 32 * 1024 * 1024 {
                    let p = GlobalLock(h);
                    if !p.is_null() {
                        out = Some(std::slice::from_raw_parts(p, sz).to_vec());
                        let _ = GlobalUnlock(h);
                    }
                }
            }
        }
        if out.is_none() && has_dib {
            let h = GetClipboardData(CF_DIB);
            if !h.is_null() {
                let sz = GlobalSize(h);
                if sz > 40 && sz <= 64 * 1024 * 1024 {
                    let p = GlobalLock(h);
                    if !p.is_null() {
                        let raw = std::slice::from_raw_parts(p, sz);
                        out = crate::cf_dib_to_png(raw).ok();
                        let _ = GlobalUnlock(h);
                    }
                }
            }
        }

        let _ = CloseClipboard();
        let post_seq = GetClipboardSequenceNumber();
        if post_seq != 0 {
            LAST_CLIPBOARD_READ_SEQ.store(post_seq, Ordering::Relaxed);
        }
        out
    }
}

unsafe fn alloc_clipboard_global(bytes: &[u8]) -> Option<HANDLE> {
    let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
    if h.is_null() {
        return None;
    }
    let ptr = GlobalLock(h);
    if ptr.is_null() {
        let _ = GlobalFree(h);
        return None;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
    let _ = GlobalUnlock(h);
    Some(h)
}

pub fn write_clipboard_png(png_bytes: &[u8]) -> Result<()> {
    let dib_bytes = crate::png_to_cf_dib(png_bytes)?;
    unsafe {
        ensure!(OpenClipboard(std::ptr::null_mut()) != 0, "打开 Windows 剪贴板失败");
        let _ = EmptyClipboard();

        let png_fmt = png_clipboard_format();
        if png_fmt != 0 {
            if let Some(h_png) = alloc_clipboard_global(png_bytes) {
                if SetClipboardData(png_fmt, h_png).is_null() {
                    let _ = GlobalFree(h_png);
                }
            }
        }

        let mut wrote_dib = false;
        if let Some(h_dib) = alloc_clipboard_global(&dib_bytes) {
            if SetClipboardData(CF_DIB, h_dib).is_null() {
                let _ = GlobalFree(h_dib);
            } else {
                wrote_dib = true;
            }
        }

        let _ = CloseClipboard();
        let post_seq = GetClipboardSequenceNumber();
        if post_seq != 0 {
            LAST_CLIPBOARD_READ_SEQ.store(post_seq, Ordering::Relaxed);
        }
        ensure!(wrote_dib, "写入 Windows 剪贴板图像失败");
        Ok(())
    }
}

pub fn trigger_internal_deskflow_slot(slot: u8) -> Result<()> {
    let vk: WORD = match slot {
        1 => VK_F18,
        2 => VK_F19,
        3 => VK_F20,
        _ => bail!("不支持的内部切屏槽位: {slot}"),
    };
    let down = INPUT {
        r#type: INPUT_KEYBOARD,
        u: INPUT_UNION {
            ki: KEYBDINPUT {
                w_vk: vk,
                w_scan: 0,
                dw_flags: 0,
                time: 0,
                dw_extra_info: 0,
            },
        },
    };
    let up = INPUT {
        r#type: INPUT_KEYBOARD,
        u: INPUT_UNION {
            ki: KEYBDINPUT {
                w_vk: vk,
                w_scan: 0,
                dw_flags: KEYEVENTF_KEYUP,
                time: 0,
                dw_extra_info: 0,
            },
        },
    };
    unsafe {
        let sent1 = SendInput(1, &down, std::mem::size_of::<INPUT>() as i32);
        thread::sleep(Duration::from_millis(15));
        let sent2 = SendInput(1, &up, std::mem::size_of::<INPUT>() as i32);
        ensure!(sent1 == 1 && sent2 == 1, "SendInput 触发内部切屏热键失败");
    }
    Ok(())
}

pub fn is_edge_locked() -> bool {
    is_being_remotely_controlled()
}

pub fn kill_orphan_deskflow_processes() {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/IM", "deskflow-core.exe"])
        .creation_flags(0x0800_0000)
        .status();
}

pub fn probe_connected_monitor() -> ConnectedMonitorProbe {
    if let Some(active_phl) = query_philips_display_config(QDC_ONLY_ACTIVE_PATHS) {
        return active_phl;
    }
    if let Some(db_phl) = query_philips_display_config(QDC_DATABASE_CURRENT) {
        return db_phl;
    }
    ConnectedMonitorProbe::default()
}

fn query_philips_display_config(flags: u32) -> Option<ConnectedMonitorProbe> {
    unsafe {
        let mut num_paths: u32 = 0;
        let mut num_modes: u32 = 0;
        if GetDisplayConfigBufferSizes(flags, &mut num_paths, &mut num_modes) != 0 || num_paths == 0
        {
            return None;
        }

        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); num_paths as usize];
        let mut modes = vec![
            DISPLAYCONFIG_MODE_INFO {
                info_type: 0,
                id: 0,
                adapter_id: LUID::default(),
                payload: [0; 12],
            };
            num_modes as usize
        ];

        let mut topo_id: u32 = 0;
        let topo_ptr = if flags == QDC_DATABASE_CURRENT {
            &mut topo_id as *mut u32
        } else {
            std::ptr::null_mut()
        };

        if QueryDisplayConfig(
            flags,
            &mut num_paths,
            paths.as_mut_ptr(),
            &mut num_modes,
            modes.as_mut_ptr(),
            topo_ptr,
        ) != 0
        {
            return None;
        }

        for p in &paths[..num_paths as usize] {
            let mut tgt: DISPLAYCONFIG_TARGET_DEVICE_NAME = std::mem::zeroed();
            tgt.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            tgt.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            tgt.header.adapter_id = p.target_info.adapter_id;
            tgt.header.id = p.target_info.id;
            if DisplayConfigGetDeviceInfo(&mut tgt.header) != 0 {
                continue;
            }

            let dev_path = utf16_prefix_to_string(&tgt.monitor_device_path);
            let friendly = utf16_prefix_to_string(&tgt.monitor_friendly_device_name);
            let path_upper = dev_path.to_ascii_uppercase();

            // Strictly match shared Philips 27E2N6900RW (PHLC44D) so secondary monitors (e.g. Dell S2719DGF) never pollute port detection
            let is_philips = path_upper.contains("PHLC44D") || friendly.contains("27E2N6900");
            if !is_philips {
                continue;
            }

            let tech = if tgt.output_technology != 0 {
                tgt.output_technology
            } else {
                p.target_info.output_technology
            };

            let (detected_input, port_label) = match tech {
                DISPLAYCONFIG_OUTPUT_TECHNOLOGY_HDMI => (Some(17), Some("HDMI 2.1".to_string())),
                DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EXTERNAL => {
                    (Some(15), Some("DP 1.4".to_string()))
                }
                DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_USB_TUNNEL => {
                    (Some(21), Some("USB-C".to_string()))
                }
                _ => (None, None),
            };

            let mut active_width = None;
            let mut active_height = None;
            let mut active_hz = None;
            if p.target_info.refresh_rate.denominator > 0 {
                let hz = (p.target_info.refresh_rate.numerator as f64
                    / p.target_info.refresh_rate.denominator as f64)
                    .round() as u32;
                if hz > 0 {
                    active_hz = Some(hz);
                }
            }

            if flags == QDC_ONLY_ACTIVE_PATHS {
                let mut src_name: DISPLAYCONFIG_SOURCE_DEVICE_NAME = std::mem::zeroed();
                src_name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
                src_name.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
                src_name.header.adapter_id = p.source_info.adapter_id;
                src_name.header.id = p.source_info.id;
                if DisplayConfigGetDeviceInfo(&mut src_name.header) == 0 {
                    let mut dm: DEVMODEW = std::mem::zeroed();
                    dm.dm_size = std::mem::size_of::<DEVMODEW>() as WORD;
                    if EnumDisplaySettingsExW(
                        src_name.view_gdi_device_name.as_ptr(),
                        ENUM_CURRENT_SETTINGS,
                        &mut dm,
                        0,
                    ) != 0
                    {
                        if dm.dm_pels_width > 0 && dm.dm_pels_height > 0 {
                            active_width = Some(dm.dm_pels_width);
                            active_height = Some(dm.dm_pels_height);
                        }
                        if dm.dm_display_frequency > 1 {
                            active_hz = Some(dm.dm_display_frequency);
                        }
                    }
                }
            }

            return Some(ConnectedMonitorProbe {
                connected: flags == QDC_ONLY_ACTIVE_PATHS,
                model_name: Some(if friendly.is_empty() {
                    "27E2N6900RW".to_string()
                } else {
                    friendly
                }),
                detected_input,
                port_label,
                active_width,
                active_height,
                active_hz,
                usb_c_pd_watts: None,
            });
        }

        None
    }
}

static REMOTE_SESSION_CACHE: Mutex<Option<(std::time::Instant, bool)>> = Mutex::new(None);

/// Strictly detects when THIS Windows PC is the controlled target (`被控端`) of an active
/// incoming remote desktop session (Windows RDP `SM_REMOTESESSION` or `GameViewerServer.exe`
/// active incoming WebRTC streamer sockets).
/// Outgoing client sessions (`GameViewer.exe` controlling an office PC) and idle virtual
/// display adapters (`GameViewer Virtual Display Adapter`) are intentionally NOT matched so
/// home 3-machine seamless switching stays 100% active while remoting into an external PC.
fn is_being_remotely_controlled() -> bool {
    if unsafe { GetSystemMetrics(0x1000) != 0 } {
        return true;
    }

    let now = std::time::Instant::now();
    if let Ok(guard) = REMOTE_SESSION_CACHE.lock() {
        if let Some((ts, cached)) = *guard {
            if now.duration_since(ts) < Duration::from_millis(2400) {
                return cached;
            }
        }
    }

    let active = detect_gameviewer_server_incoming_stream();

    if let Ok(mut guard) = REMOTE_SESSION_CACHE.lock() {
        *guard = Some((now, active));
    }
    active
}

fn detect_gameviewer_server_incoming_stream() -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // 1. Find PIDs of `GameViewerServer.exe` (strictly the controlled server daemon, NEVER `GameViewer.exe` client)
    let tl_out = match std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq GameViewerServer.exe", "/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };
    let tl_text = String::from_utf8_lossy(&tl_out.stdout);
    let mut server_pids: Vec<String> = Vec::new();
    for line in tl_text.lines() {
        let cols: Vec<&str> = line.split(',').map(|s| s.trim().trim_matches('"')).collect();
        if cols.len() >= 2 && cols[0].eq_ignore_ascii_case("GameViewerServer.exe") {
            if !cols[1].is_empty() && cols[1].chars().all(|c| c.is_ascii_digit()) {
                server_pids.push(cols[1].to_string());
            }
        }
    }
    if server_pids.is_empty() {
        return false;
    }

    // 2. When `GameViewerServer.exe` is idle, it only binds `127.0.0.1` TCP listeners and `0.0.0.0` wildcard UDP.
    // When actively being remotely controlled (`peer_control_connection: connected`), its WebRTC streamer binds
    // non-loopback interface TCP `LISTENING` sockets (e.g. `192.168.x.x:port`, `100.x.x.x:port`) and
    // interface-specific UDP sockets.
    let ns_out = match std::process::Command::new("netstat")
        .args(["-ano"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };
    let ns_text = String::from_utf8_lossy(&ns_out.stdout);
    for line in ns_text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let pid = parts[parts.len() - 1];
        if !server_pids.iter().any(|p| p == pid) {
            continue;
        }
        let proto = parts[0];
        let local_addr = parts[1];
        let host_part = local_addr
            .rsplit_once(':')
            .map(|(h, _)| h)
            .unwrap_or(local_addr);
        let is_wildcard_or_loopback = host_part == "0.0.0.0"
            || host_part == "127.0.0.1"
            || host_part == "[::]"
            || host_part == "[::1]"
            || host_part == "*";

        if proto.eq_ignore_ascii_case("TCP") && parts.len() >= 5 {
            let state = parts[3];
            if state.eq_ignore_ascii_case("LISTENING") && !is_wildcard_or_loopback {
                return true;
            }
        } else if proto.eq_ignore_ascii_case("UDP") && !is_wildcard_or_loopback {
            return true;
        }
    }

    false
}

fn find_target_gdi_device_name() -> Option<[u16; 32]> {
    unsafe {
        let mut num_paths: u32 = 0;
        let mut num_modes: u32 = 0;
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut num_paths, &mut num_modes) == 0
            && num_paths > 0
        {
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); num_paths as usize];
            let mut modes = vec![
                DISPLAYCONFIG_MODE_INFO {
                    info_type: 0,
                    id: 0,
                    adapter_id: LUID::default(),
                    payload: [0; 12],
                };
                num_modes as usize
            ];
            if QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut num_paths,
                paths.as_mut_ptr(),
                &mut num_modes,
                modes.as_mut_ptr(),
                std::ptr::null_mut(),
            ) == 0
            {
                for p in &paths[..num_paths as usize] {
                    let mut tgt: DISPLAYCONFIG_TARGET_DEVICE_NAME = std::mem::zeroed();
                    tgt.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
                    tgt.header.size =
                        std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
                    tgt.header.adapter_id = p.target_info.adapter_id;
                    tgt.header.id = p.target_info.id;
                    if DisplayConfigGetDeviceInfo(&mut tgt.header) != 0 {
                        continue;
                    }
                    let dev_path =
                        utf16_prefix_to_string(&tgt.monitor_device_path).to_ascii_uppercase();
                    let friendly = utf16_prefix_to_string(&tgt.monitor_friendly_device_name);
                    if dev_path.contains("PHLC44D") || friendly.contains("27E2N6900") {
                        let mut src_name: DISPLAYCONFIG_SOURCE_DEVICE_NAME = std::mem::zeroed();
                        src_name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
                        src_name.header.size =
                            std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
                        src_name.header.adapter_id = p.source_info.adapter_id;
                        src_name.header.id = p.source_info.id;
                        if DisplayConfigGetDeviceInfo(&mut src_name.header) == 0 {
                            return Some(src_name.view_gdi_device_name);
                        }
                    }
                }
            }
        }

        let mut dev_idx = 0;
        loop {
            let mut dev: DISPLAY_DEVICEW = std::mem::zeroed();
            dev.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as DWORD;
            if EnumDisplayDevicesW(std::ptr::null(), dev_idx, &mut dev, 0) == 0 {
                break;
            }
            dev_idx += 1;
            if (dev.state_flags & DISPLAY_DEVICE_ACTIVE) == 0 {
                continue;
            }
            let dev_str = utf16_prefix_to_string(&dev.device_string).to_ascii_lowercase();
            if dev_str.contains("gameviewer") || dev_str.contains("virtual") {
                continue;
            }
            return Some(dev.device_name);
        }
    }
    None
}

pub fn list_display_modes(_prefer_external: bool) -> Vec<DisplayModeOption> {
    let Some(dev_name) = find_target_gdi_device_name() else {
        return Vec::new();
    };
    unsafe {
        let mut cur: DEVMODEW = std::mem::zeroed();
        cur.dm_size = std::mem::size_of::<DEVMODEW>() as WORD;
        let has_cur =
            EnumDisplaySettingsExW(dev_name.as_ptr(), ENUM_CURRENT_SETTINGS, &mut cur, 0) != 0;

        let mut raw: Vec<DisplayModeOption> = Vec::new();
        let mut mode_idx: DWORD = 0;
        loop {
            let mut m: DEVMODEW = std::mem::zeroed();
            m.dm_size = std::mem::size_of::<DEVMODEW>() as WORD;
            if EnumDisplaySettingsExW(dev_name.as_ptr(), mode_idx, &mut m, 0) == 0 {
                break;
            }
            let idx = mode_idx as i32;
            mode_idx += 1;
            if m.dm_pels_width < 1280 || m.dm_pels_height < 720 || m.dm_display_frequency < 59 {
                continue;
            }
            let is_active = has_cur
                && m.dm_pels_width == cur.dm_pels_width
                && m.dm_pels_height == cur.dm_pels_height
                && m.dm_display_frequency == cur.dm_display_frequency;
            let label = format!(
                "{}×{} · {}Hz",
                m.dm_pels_width, m.dm_pels_height, m.dm_display_frequency
            );
            raw.push(DisplayModeOption {
                mode_id: idx,
                width: m.dm_pels_width,
                height: m.dm_pels_height,
                pixel_width: m.dm_pels_width,
                pixel_height: m.dm_pels_height,
                hz: m.dm_display_frequency,
                hidpi: false,
                current: is_active,
                is_builtin: false,
                label,
            });
        }

        raw.sort_by(|a, b| {
            let area_a = u64::from(a.width) * u64::from(a.height);
            let area_b = u64::from(b.width) * u64::from(b.height);
            area_b
                .cmp(&area_a)
                .then_with(|| b.hz.cmp(&a.hz))
                .then_with(|| b.current.cmp(&a.current))
        });

        let mut deduped: Vec<DisplayModeOption> = Vec::new();
        for item in raw {
            if let Some(existing) = deduped
                .iter_mut()
                .find(|x| x.width == item.width && x.height == item.height && x.hz == item.hz)
            {
                if item.current {
                    *existing = item;
                }
                continue;
            }
            // Keep max 2 refresh rates per resolution (plus active mode) so the menu stays clean
            let same_res_count = deduped
                .iter()
                .filter(|x| x.width == item.width && x.height == item.height)
                .count();
            if same_res_count >= 2 && !item.current {
                continue;
            }
            deduped.push(item);
            if deduped.len() >= 10 {
                break;
            }
        }
        deduped
    }
}

pub fn apply_display_mode(
    _mode_id: i32,
    width: u32,
    height: u32,
    hz: u32,
    _hidpi: bool,
    prefer_external: bool,
) -> Result<DisplayModeOption> {
    let Some(dev_name) = find_target_gdi_device_name() else {
        bail!("未找到可切换分辨率的 Windows 显示器");
    };
    unsafe {
        let mut target_mode: Option<DEVMODEW> = None;
        let mut mode_idx: DWORD = 0;
        loop {
            let mut m: DEVMODEW = std::mem::zeroed();
            m.dm_size = std::mem::size_of::<DEVMODEW>() as WORD;
            if EnumDisplaySettingsExW(dev_name.as_ptr(), mode_idx, &mut m, 0) == 0 {
                break;
            }
            mode_idx += 1;
            if m.dm_pels_width == width && m.dm_pels_height == height {
                if hz == 0 || m.dm_display_frequency == hz {
                    target_mode = Some(m);
                    break;
                }
                if target_mode.is_none() {
                    target_mode = Some(m);
                }
            }
        }
        let Some(mut best) = target_mode else {
            bail!("Windows 显示器不支持目标分辨率 {}×{}@{}Hz", width, height, hz);
        };
        if best.dm_bits_per_pel < 32 {
            best.dm_bits_per_pel = 32;
        }
        best.dm_fields = DM_BITSPERPEL | DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
        let mut rc = ChangeDisplaySettingsExW(
            dev_name.as_ptr(),
            &mut best,
            std::ptr::null_mut(),
            CDS_UPDATEREGISTRY,
            std::ptr::null_mut(),
        );
        if rc != 0 {
            rc = ChangeDisplaySettingsExW(
                dev_name.as_ptr(),
                &mut best,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
            );
        }
        ensure!(rc == 0, "ChangeDisplaySettingsExW 切换分辨率失败: rc={rc}");
    }
    let updated = list_display_modes(prefer_external);
    updated
        .into_iter()
        .find(|m| m.current)
        .or_else(|| {
            Some(DisplayModeOption {
                mode_id: 0,
                width,
                height,
                pixel_width: width,
                pixel_height: height,
                hz,
                hidpi: false,
                current: true,
                is_builtin: false,
                label: format!("{}×{} · {}Hz", width, height, hz),
            })
        })
        .ok_or_else(|| anyhow::anyhow!("切换后读取显示模式失败"))
}

pub fn apply_software_gamma(
    _brightness_pct: u8,
    _color_preset: u8,
    _prefer_external: bool,
) -> Result<()> {
    Ok(())
}

pub fn kill_other_kvmddc_instances() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let my_pid = std::process::id();
    let filter = format!("PID ne {my_pid}");
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/FI", &filter, "/IM", "kvmddc.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}
