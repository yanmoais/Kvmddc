use crate::{
    build_ddc_get_vcp_packet, build_ddc_set_vcp_packet, parse_ddc_vcp_reply, ConnectedMonitorProbe,
    DisplayModeOption, EdgeSide, PointerSample,
};
use anyhow::{bail, ensure, Result};
use std::ffi::{c_char, c_void};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, AtomicIsize, Ordering},
    Mutex,
};
use std::thread;
use std::time::Duration;

type CFTypeRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFStringRef = *const c_void;
type IOAVServiceRef = *mut c_void;
type CGEventRef = *mut c_void;
type IoObjectT = u32;
type IoRegistryEntryT = u32;
type IoIteratorT = u32;
type KernReturnT = i32;
type CGDirectDisplayID = u32;

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_IO_REGISTRY_ITERATE_RECURSIVELY: u32 = 0x0000_0001;
const K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: i32 = 1;
const K_CG_HID_EVENT_TAP: u32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
    fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        c_str: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFStringGetTypeID() -> usize;
    fn CFStringGetCString(
        the_string: CFStringRef,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> bool;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(boolean: CFTypeRef) -> bool;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFDictionaryGetValue(the_dict: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFDictionaryCreate(
        allocator: CFAllocatorRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        num_values: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFTypeRef;
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberGetValue(number: CFTypeRef, the_type: isize, value_ptr: *mut c_void) -> bool;
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFStringCompare(s1: CFStringRef, s2: CFStringRef, compare_options: usize) -> isize;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IORegistryGetRootEntry(main_port: u32) -> IoRegistryEntryT;
    fn IORegistryEntryCreateIterator(
        entry: IoRegistryEntryT,
        plane: *const c_char,
        options: u32,
        iterator: *mut IoIteratorT,
    ) -> KernReturnT;
    fn IOIteratorNext(iterator: IoIteratorT) -> IoObjectT;
    fn IOObjectConformsTo(object: IoObjectT, class_name: *const c_char) -> u32;
    fn IOObjectRelease(object: IoObjectT) -> KernReturnT;
    fn IORegistryEntryCreateCFProperty(
        entry: IoRegistryEntryT,
        key: CFStringRef,
        allocator: CFAllocatorRef,
        options: u32,
    ) -> CFTypeRef;
    fn IOServiceMatching(name: *const c_char) -> CFTypeRef;
    fn IOServiceGetMatchingService(main_port: u32, matching: CFTypeRef) -> IoObjectT;
    fn IOServiceGetMatchingServices(
        main_port: u32,
        matching: CFTypeRef,
        existing: *mut IoIteratorT,
    ) -> KernReturnT;
    fn IOAVServiceCreateWithService(
        allocator: CFAllocatorRef,
        service: IoObjectT,
    ) -> IOAVServiceRef;
    fn IOAVServiceReadI2C(
        service: IOAVServiceRef,
        chip_address: u32,
        offset: u32,
        output_buffer: *mut u8,
        output_buffer_size: u32,
    ) -> KernReturnT;
    fn IOAVServiceWriteI2C(
        service: IOAVServiceRef,
        chip_address: u32,
        data_address: u32,
        input_buffer: *mut u8,
        input_buffer_size: u32,
    ) -> KernReturnT;
    fn IOPMAssertionCreateWithName(
        assertion_type: CFStringRef,
        assertion_level: u32,
        assertion_name: CFStringRef,
        assertion_id: *mut u32,
    ) -> KernReturnT;
    fn IOPMAssertionDeclareUserActivity(
        assertion_name: CFStringRef,
        user_type: u32,
        assertion_id: *mut u32,
    ) -> KernReturnT;
    fn IOPMAssertionRelease(assertion_id: u32) -> KernReturnT;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreate(source: *const c_void) -> CGEventRef;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
    fn CGEventSourceButtonState(state_id: i32, button: u32) -> bool;
    fn CGEventSourceSecondsSinceLastEventType(state_id: i32, event_type: u32) -> f64;
    fn CGEventCreateKeyboardEvent(
        source: *const c_void,
        virtual_key: u16,
        key_down: bool,
    ) -> CGEventRef;
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn CGEventPost(tap: u32, event: CGEventRef);
    fn CGGetDisplaysWithPoint(
        point: CGPoint,
        max_displays: u32,
        displays: *mut CGDirectDisplayID,
        matching_display_count: *mut u32,
    ) -> i32;
    fn CGGetOnlineDisplayList(
        max_displays: u32,
        online_displays: *mut CGDirectDisplayID,
        display_count: *mut u32,
    ) -> i32;
    fn CGGetActiveDisplayList(
        max_displays: u32,
        active_displays: *mut CGDirectDisplayID,
        display_count: *mut u32,
    ) -> i32;
    fn CGMainDisplayID() -> CGDirectDisplayID;
    fn CGDisplayBounds(display: CGDirectDisplayID) -> CGRect;
    fn CGWarpMouseCursorPosition(new_cursor_position: CGPoint) -> i32;
    fn CGDisplayIsBuiltin(display: CGDirectDisplayID) -> u32;
    fn CGDisplayVendorNumber(display: CGDirectDisplayID) -> u32;
    fn CGDisplayModelNumber(display: CGDirectDisplayID) -> u32;
    #[allow(dead_code)]
    fn CGDisplayIsCaptured(display: CGDirectDisplayID) -> u32;
    fn CGDisplayCopyDisplayMode(display: CGDirectDisplayID) -> CFTypeRef;
    fn CGDisplayModeRelease(mode: CFTypeRef);
    fn CGDisplayModeGetWidth(mode: CFTypeRef) -> usize;
    fn CGDisplayModeGetHeight(mode: CFTypeRef) -> usize;
    fn CGDisplayModeGetPixelWidth(mode: CFTypeRef) -> usize;
    fn CGDisplayModeGetPixelHeight(mode: CFTypeRef) -> usize;
    fn CGDisplayModeGetRefreshRate(mode: CFTypeRef) -> f64;
    fn CGBeginDisplayConfiguration(config: *mut *mut c_void) -> i32;
    fn CGCompleteDisplayConfiguration(config: *mut c_void, option: u32) -> i32;
    fn CGCancelDisplayConfiguration(config: *mut c_void) -> i32;
    fn CGSGetNumberOfDisplayModes(display: CGDirectDisplayID, n_modes: *mut i32) -> i32;
    fn CGSGetCurrentDisplayMode(display: CGDirectDisplayID, mode_num: *mut i32) -> i32;
    fn CGSGetDisplayModeDescriptionOfLength(
        display: CGDirectDisplayID,
        idx: i32,
        mode: *mut u8,
        length: i32,
    ) -> i32;
    fn CGSConfigureDisplayMode(
        config: *mut c_void,
        display: CGDirectDisplayID,
        mode_num: i32,
    ) -> i32;
    fn CGSetDisplayTransferByTable(
        display: CGDirectDisplayID,
        table_size: u32,
        red_table: *const f32,
        green_table: *const f32,
        blue_table: *const f32,
    ) -> i32;
    fn CGDisplayRestoreColorSyncSettings();
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {}

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
}

extern "C" {
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

static SAVED_BUILTIN_BRIGHTNESS: Mutex<Option<f32>> = Mutex::new(None);
static REMOTE_SESSION_CACHE: Mutex<Option<(std::time::Instant, bool)>> = Mutex::new(None);
static DISPLAY_AWAKE_ASSERTION_ID: Mutex<Option<u32>> = Mutex::new(None);
static USER_WAKE_ASSERTION_ID: Mutex<u32> = Mutex::new(0);

type DisplayServicesGetBrightnessFn = unsafe extern "C" fn(u32, *mut f32) -> i32;
type DisplayServicesSetBrightnessFn = unsafe extern "C" fn(u32, f32) -> i32;

fn load_display_services() -> Option<(DisplayServicesGetBrightnessFn, DisplayServicesSetBrightnessFn)>
{
    unsafe {
        let handle = dlopen(
            b"/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices\0"
                .as_ptr() as *const c_char,
            0x1, // RTLD_LAZY
        );
        if handle.is_null() {
            return None;
        }
        let get_ptr = dlsym(
            handle,
            b"DisplayServicesGetBrightness\0".as_ptr() as *const c_char,
        );
        let set_ptr = dlsym(
            handle,
            b"DisplayServicesSetBrightness\0".as_ptr() as *const c_char,
        );
        if get_ptr.is_null() || set_ptr.is_null() {
            return None;
        }
        Some((
            std::mem::transmute::<*mut c_void, DisplayServicesGetBrightnessFn>(get_ptr),
            std::mem::transmute::<*mut c_void, DisplayServicesSetBrightnessFn>(set_ptr),
        ))
    }
}

struct CfGuard(CFTypeRef);
impl Drop for CfGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

fn make_cfstring(cstr: &[u8]) -> Result<CfGuard> {
    let ptr = unsafe {
        CFStringCreateWithCString(
            std::ptr::null(),
            cstr.as_ptr() as *const c_char,
            K_CF_STRING_ENCODING_UTF8,
        )
    };
    ensure!(!ptr.is_null(), "创建 CFString 失败");
    Ok(CfGuard(ptr))
}

fn open_external_av_service() -> Result<CfGuard> {
    unsafe {
        let root = IORegistryGetRootEntry(0);
        ensure!(root != 0, "无法读取 IORegistry 根节点");

        let mut iter: IoIteratorT = 0;
        let kr = IORegistryEntryCreateIterator(
            root,
            b"IOService\0".as_ptr() as *const c_char,
            K_IO_REGISTRY_ITERATE_RECURSIVELY,
            &mut iter,
        );
        IOObjectRelease(root);
        ensure!(kr == 0 && iter != 0, "创建 IORegistry 迭代器失败: {kr}");

        let cf_loc = make_cfstring(b"Location\0")?;
        let cf_ext = make_cfstring(b"External\0")?;

        let mut found_av: IOAVServiceRef = std::ptr::null_mut();
        loop {
            let entry = IOIteratorNext(iter);
            if entry == 0 {
                break;
            }
            let is_dcp =
                IOObjectConformsTo(entry, b"DCPAVServiceProxy\0".as_ptr() as *const c_char) != 0;
            if is_dcp {
                let prop =
                    IORegistryEntryCreateCFProperty(entry, cf_loc.0, std::ptr::null(), 0);
                let mut is_external = false;
                if !prop.is_null() {
                    if CFGetTypeID(prop) == CFStringGetTypeID()
                        && CFStringCompare(prop, cf_ext.0, 0) == 0
                    {
                        is_external = true;
                    }
                    CFRelease(prop);
                }
                if is_external {
                    let av = IOAVServiceCreateWithService(std::ptr::null(), entry);
                    if !av.is_null() {
                        found_av = av;
                        IOObjectRelease(entry);
                        break;
                    }
                }
            }
            IOObjectRelease(entry);
        }
        IOObjectRelease(iter);

        if found_av.is_null() {
            bail!("未检测到飞利浦外接屏，请开启显示器电源或检查视频线");
        }
        Ok(CfGuard(found_av as CFTypeRef))
    }
}

pub fn has_external_display() -> bool {
    open_external_av_service().is_ok() || probe_connected_monitor().connected
}

pub fn is_accessibility_trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

pub fn prompt_and_open_accessibility_settings() -> Result<bool> {
    let trusted = unsafe {
        let keys = [kAXTrustedCheckOptionPrompt as CFTypeRef];
        let vals = [kCFBooleanTrue];
        let dict = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            vals.as_ptr(),
            1,
            std::ptr::null(),
            std::ptr::null(),
        );
        let res = if !dict.is_null() {
            let t = AXIsProcessTrustedWithOptions(dict) != 0;
            CFRelease(dict);
            t
        } else {
            AXIsProcessTrusted() != 0
        };
        res
    };

    let _ = Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();

    Ok(trusted)
}

unsafe fn cf_string_to_rust(cf: CFTypeRef) -> Option<String> {
    if cf.is_null() || CFGetTypeID(cf) != CFStringGetTypeID() {
        return None;
    }
    let mut buf = [0u8; 256];
    if CFStringGetCString(
        cf,
        buf.as_mut_ptr() as *mut c_char,
        buf.len() as isize,
        K_CF_STRING_ENCODING_UTF8,
    ) {
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..end]).trim().to_string())
    } else {
        None
    }
}

unsafe fn cf_number_to_i64(cf: CFTypeRef) -> Option<i64> {
    if cf.is_null() || CFGetTypeID(cf) != CFNumberGetTypeID() {
        return None;
    }
    let mut val: i64 = 0;
    // kCFNumberSInt64Type = 4
    if CFNumberGetValue(cf, 4, &mut val as *mut i64 as *mut c_void) {
        Some(val)
    } else {
        None
    }
}

unsafe fn dict_get(dict: CFTypeRef, key: &[u8]) -> CFTypeRef {
    if dict.is_null() || CFGetTypeID(dict) != CFDictionaryGetTypeID() {
        return std::ptr::null();
    }
    let Ok(k) = make_cfstring(key) else {
        return std::ptr::null();
    };
    CFDictionaryGetValue(dict, k.0)
}

/// Checks whether Philips 27E2N6900RW internal USB hub (VID 0x0552 = 1362) is enumerated on USB-C.
fn has_philips_usb_upstream_hub() -> bool {
    unsafe {
        let root = IORegistryGetRootEntry(0);
        if root == 0 {
            return false;
        }
        let mut iter: IoIteratorT = 0;
        let kr = IORegistryEntryCreateIterator(
            root,
            b"IOService\0".as_ptr() as *const c_char,
            K_IO_REGISTRY_ITERATE_RECURSIVELY,
            &mut iter,
        );
        IOObjectRelease(root);
        if kr != 0 || iter == 0 {
            return false;
        }
        let Ok(cf_vid) = make_cfstring(b"idVendor\0") else {
            IOObjectRelease(iter);
            return false;
        };
        let mut found = false;
        loop {
            let entry = IOIteratorNext(iter);
            if entry == 0 {
                break;
            }
            if IOObjectConformsTo(entry, b"IOUSBHostDevice\0".as_ptr() as *const c_char) != 0 {
                let prop = IORegistryEntryCreateCFProperty(entry, cf_vid.0, std::ptr::null(), 0);
                if !prop.is_null() {
                    if cf_number_to_i64(prop) == Some(1362) {
                        found = true;
                    }
                    CFRelease(prop);
                }
            }
            IOObjectRelease(entry);
            if found {
                break;
            }
        }
        IOObjectRelease(iter);
        found
    }
}

fn is_virtual_headless_display_id(did: CGDirectDisplayID) -> bool {
    let (vendor, model) = unsafe { (CGDisplayVendorNumber(did), CGDisplayModelNumber(did)) };
    (vendor == 2198 && model == 2878)
        || (vendor == 0x756e6b6e && model == 0x76697274)
        || (vendor == 21828 && model == 20993)
}

pub fn probe_connected_monitor() -> ConnectedMonitorProbe {
    let mut found_ext_fb = false;
    let mut model_name: Option<String> = None;
    let mut downstream_transport: Option<String> = None;
    let mut has_hdmi_edid = false;
    let mut max_hz_attr: Option<u32> = None;

    unsafe {
        let root = IORegistryGetRootEntry(0);
        if root != 0 {
            let mut iter: IoIteratorT = 0;
            let kr = IORegistryEntryCreateIterator(
                root,
                b"IOService\0".as_ptr() as *const c_char,
                K_IO_REGISTRY_ITERATE_RECURSIVELY,
                &mut iter,
            );
            IOObjectRelease(root);
            if kr == 0 && iter != 0 {
                let cf_disp_attr = make_cfstring(b"DisplayAttributes\0").ok();
                let cf_trans = make_cfstring(b"Transport\0").ok();

                loop {
                    let entry = IOIteratorNext(iter);
                    if entry == 0 {
                        break;
                    }
                    if let Some(ref k_da) = cf_disp_attr {
                        let da_dict =
                            IORegistryEntryCreateCFProperty(entry, k_da.0, std::ptr::null(), 0);
                        if !da_dict.is_null() {
                            let prod_dict = dict_get(da_dict, b"ProductAttributes\0");
                            let pname = cf_string_to_rust(dict_get(prod_dict, b"ProductName\0"));
                            let mfg = cf_string_to_rust(dict_get(prod_dict, b"ManufacturerID\0"));
                            let is_builtin_apple = mfg.as_deref() == Some("APP")
                                && !pname
                                    .as_deref()
                                    .is_some_and(|n| n.contains("Studio") || n.contains("Pro Display"));
                            let is_virtual_display = pname
                                .as_deref()
                                .is_some_and(|n| n == "Mac Retina P3" || n.contains("UU远程"));
                            if !is_builtin_apple && !is_virtual_display && (mfg.is_some() || pname.is_some()) {
                                found_ext_fb = true;
                                if let Some(pn) = pname {
                                    if !pn.is_empty() {
                                        model_name = Some(pn);
                                    }
                                }
                                if let Some(ref k_tr) = cf_trans {
                                    let tr_dict = IORegistryEntryCreateCFProperty(
                                        entry,
                                        k_tr.0,
                                        std::ptr::null(),
                                        0,
                                    );
                                    if !tr_dict.is_null() {
                                        let ds = dict_get(tr_dict, b"Downstream\0");
                                        if let Some(s) = cf_string_to_rust(ds) {
                                            downstream_transport = Some(s);
                                        }
                                        CFRelease(tr_dict);
                                    }
                                }
                                let hdmi_flag = dict_get(da_dict, b"HasHDMILegacyEDID\0");
                                if !hdmi_flag.is_null()
                                    && CFGetTypeID(hdmi_flag) == CFBooleanGetTypeID()
                                {
                                    has_hdmi_edid = CFBooleanGetValue(hdmi_flag);
                                }
                                if let Some(mhz) =
                                    cf_number_to_i64(dict_get(da_dict, b"MaximumRefreshRate\0"))
                                {
                                    if mhz > 0 && mhz <= 600 {
                                        max_hz_attr = Some(mhz as u32);
                                    }
                                }
                                let is_phl = mfg.as_deref() == Some("PHL")
                                    || model_name
                                        .as_deref()
                                        .is_some_and(|n| n.contains("27E2N6900"));
                                CFRelease(da_dict);
                                if is_phl {
                                    IOObjectRelease(entry);
                                    break;
                                }
                            } else {
                                CFRelease(da_dict);
                            }
                        }
                    }
                    IOObjectRelease(entry);
                }
                IOObjectRelease(iter);
            }
        }
    }

    // Read online external display resolution & refresh rate from CoreGraphics (works even when display asleep!)
    let mut found_online_cg = false;
    let mut active_width = None;
    let mut active_height = None;
    let mut active_hz = max_hz_attr;

    unsafe {
        let mut ids = [0u32; 16];
        let mut cnt = 0u32;
        if CGGetOnlineDisplayList(16, ids.as_mut_ptr(), &mut cnt) == 0 {
            let mut chosen: Option<u32> = None;
            for &did in &ids[..cnt as usize] {
                if CGDisplayIsBuiltin(did) == 0 && !is_virtual_headless_display_id(did) {
                    // Ignore dummy/virtual displays without vendor or pick Philips 0x410C (16652)
                    let vendor = CGDisplayVendorNumber(did);
                    if vendor == 16652 || (chosen.is_none() && found_ext_fb) {
                        chosen = Some(did);
                        if vendor == 16652 {
                            break;
                        }
                    }
                }
            }
            if let Some(did) = chosen {
                found_online_cg = true;
                let mode = CGDisplayCopyDisplayMode(did);
                if !mode.is_null() {
                    let lw = CGDisplayModeGetWidth(mode) as u32;
                    let lh = CGDisplayModeGetHeight(mode) as u32;
                    let pw = CGDisplayModeGetPixelWidth(mode) as u32;
                    let ph = CGDisplayModeGetPixelHeight(mode) as u32;
                    let hz = CGDisplayModeGetRefreshRate(mode).round() as u32;
                    if lw > 0 && lh > 0 {
                        active_width = Some(lw);
                        active_height = Some(lh);
                    } else if pw > 0 && ph > 0 {
                        active_width = Some(pw);
                        active_height = Some(ph);
                    }
                    if hz > 0 {
                        active_hz = Some(hz);
                    }
                    CGDisplayModeRelease(mode);
                }
            }
        }
    }

    if !found_ext_fb && !found_online_cg && open_external_av_service().is_err() {
        return ConnectedMonitorProbe::default();
    }

    let is_hdmi = has_hdmi_edid
        || downstream_transport
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case("HDMI"));
    let usb_hub = if is_hdmi {
        false
    } else {
        has_philips_usb_upstream_hub()
    };

    let (detected_input, port_label, usb_c_pd_watts) = if is_hdmi {
        (Some(17), Some("HDMI 2.1".to_string()), None)
    } else if usb_hub {
        (Some(21), Some("USB-C".to_string()), Some(96))
    } else {
        // USB-C DP Alt-Mode drops the monitor's internal USB hub when the OSD switches to
        // another input; avoid misidentifying an inactive USB-C link as native DP 1.4 (15).
        (None, None, None)
    };

    ConnectedMonitorProbe {
        connected: true,
        model_name,
        detected_input,
        port_label,
        active_width,
        active_height,
        active_hz,
        usb_c_pd_watts,
    }
}

pub fn read_vcp(vcp_code: u8) -> Result<(u16, u16)> {
    let av = open_external_av_service()?;
    let mut last_err = None;

    for attempt in 0..2 {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(30));
        }
        let mut req = build_ddc_get_vcp_packet(vcp_code);
        thread::sleep(Duration::from_millis(10));
        let wkr = unsafe {
            IOAVServiceWriteI2C(
                av.0 as IOAVServiceRef,
                0x37,
                0x51,
                req.as_mut_ptr(),
                req.len() as u32,
            )
        };
        if wkr != 0 {
            last_err = Some(anyhow::anyhow!("IOAVServiceWriteI2C 请求失败 (kr=0x{wkr:X})"));
            continue;
        }

        thread::sleep(Duration::from_millis(45));
        let mut reply = [0u8; 12];
        let rkr = unsafe {
            IOAVServiceReadI2C(
                av.0 as IOAVServiceRef,
                0x37,
                0x51,
                reply.as_mut_ptr(),
                reply.len() as u32,
            )
        };
        if rkr != 0 {
            last_err = Some(anyhow::anyhow!("IOAVServiceReadI2C 读取失败 (kr=0x{rkr:X})"));
            continue;
        }

        match parse_ddc_vcp_reply(&reply, vcp_code) {
            Ok(pair) => return Ok(pair),
            Err(e) => last_err = Some(e),
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("DDC 读取未知错误")))
}

pub fn write_vcp(vcp_code: u8, value: u16) -> Result<()> {
    let av = open_external_av_service()?;
    let mut pkt = build_ddc_set_vcp_packet(vcp_code, value);
    thread::sleep(Duration::from_millis(10));
    let kr = unsafe {
        IOAVServiceWriteI2C(
            av.0 as IOAVServiceRef,
            0x37,
            0x51,
            pkt.as_mut_ptr(),
            pkt.len() as u32,
        )
    };
    ensure!(
        kr == 0,
        "IOAVServiceWriteI2C 写入 0x{vcp_code:02X}={value} 失败 (kr=0x{kr:X})"
    );
    Ok(())
}

pub fn sleep_displays_keep_awake() -> Result<()> {
    // Intentionally avoid `pmset displaysleepnow`: on macOS it immediately triggers loginwindow
    // Screen Lock AND gets bounced back awake within 0.5s when keep-awake daemons (e.g. UURemoteServer)
    // hold PreventUserIdleDisplaySleep. Instead, zero built-in XDR backlight hardware directly while
    // AppRuntime pairs this with NSScreenSaverWindowLevel+1 pure-black blankers + external DDC 0%.
    if let Some((get_fn, set_fn)) = load_display_services() {
        unsafe {
            let mut ids = [0u32; 16];
            let mut cnt = 0u32;
            if CGGetOnlineDisplayList(16, ids.as_mut_ptr(), &mut cnt) == 0 {
                for &did in &ids[..cnt as usize] {
                    if CGDisplayIsBuiltin(did) != 0 {
                        let mut cur: f32 = -1.0;
                        if get_fn(did, &mut cur) == 0 && cur > 0.02 {
                            if let Ok(mut guard) = SAVED_BUILTIN_BRIGHTNESS.lock() {
                                *guard = Some(cur);
                            }
                        }
                        let _ = set_fn(did, 0.0);
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn wake_displays_and_wait() -> Result<bool> {
    let _ = Command::new("/usr/bin/caffeinate")
        .args(["-u", "-t", "2"])
        .spawn();

    if let Some((_get_fn, set_fn)) = load_display_services() {
        let saved = SAVED_BUILTIN_BRIGHTNESS
            .lock()
            .ok()
            .and_then(|mut g| g.take());
        let restore_val = saved.unwrap_or(0.75).clamp(0.15, 1.0);
        unsafe {
            let mut ids = [0u32; 16];
            let mut cnt = 0u32;
            if CGGetOnlineDisplayList(16, ids.as_mut_ptr(), &mut cnt) == 0 {
                for &did in &ids[..cnt as usize] {
                    if CGDisplayIsBuiltin(did) != 0 {
                        let _ = set_fn(did, restore_val);
                    }
                }
            }
        }
    }

    Ok(true)
}

/// Detects any genuine keyboard press (including modifier keys) or mouse button click
/// after standby was entered, ignoring mouse movement drift and UURemote power assertions.
pub fn has_recent_wake_input(elapsed_since_sleep: f64) -> bool {
    // 1 = LeftMouseDown, 3 = RightMouseDown, 10 = KeyDown, 12 = FlagsChanged, 25 = OtherMouseDown
    const WAKE_EVENTS: [u32; 5] = [1, 3, 10, 12, 25];
    unsafe {
        for &ev in &WAKE_EVENTS {
            for state_id in [0i32, 1i32] {
                let secs = CGEventSourceSecondsSinceLastEventType(state_id, ev);
                if secs.is_finite()
                    && secs >= 0.0
                    && secs < 0.28
                    && (secs + 0.30) < elapsed_since_sleep
                {
                    return true;
                }
            }
        }
    }
    false
}

pub fn heal_display_profile(
    _preferred_width: u32,
    _preferred_height: u32,
    _preferred_hz: u32,
) -> Result<Option<String>> {
    // macOS natively persists per-EDID HiDPI scaling (2560x1440@75Hz) & Display P3 profile;
    // hardware brightness guardian is enforced centrally by runtime controller.
    Ok(None)
}

fn select_target_cg_display(prefer_external: bool) -> Option<(CGDirectDisplayID, bool)> {
    unsafe {
        let mut ids = [0u32; 16];
        let mut cnt = 0u32;
        if CGGetOnlineDisplayList(16, ids.as_mut_ptr(), &mut cnt) != 0 || cnt == 0 {
            return None;
        }
        let mut ext_id: Option<u32> = None;
        let mut builtin_id: Option<u32> = None;
        for &did in &ids[..cnt as usize] {
            if CGDisplayIsBuiltin(did) != 0 {
                if builtin_id.is_none() {
                    builtin_id = Some(did);
                }
            } else {
                let vendor = CGDisplayVendorNumber(did);
                if vendor == 16652 {
                    ext_id = Some(did);
                    break;
                } else if vendor == 2198 {
                    ext_id = Some(did);
                } else if ext_id.is_none() {
                    ext_id = Some(did);
                }
            }
        }
        if prefer_external {
            if let Some(did) = ext_id {
                return Some((did, false));
            }
        }
        if let Some(did) = builtin_id {
            return Some((did, true));
        }
        ext_id.map(|did| (did, false))
    }
}

#[derive(Clone)]
struct RawCgsMode {
    mode_id: i32,
    width: u32,
    height: u32,
    pixel_width: u32,
    pixel_height: u32,
    depth: u32,
    hz: u32,
    hidpi: bool,
    current: bool,
}

fn read_cgs_modes_for_display(did: CGDirectDisplayID) -> Vec<RawCgsMode> {
    unsafe {
        let mut n_modes: i32 = 0;
        if CGSGetNumberOfDisplayModes(did, &mut n_modes) != 0 || n_modes <= 0 {
            return Vec::new();
        }
        let mut cur_mode_id: i32 = -1;
        let _ = CGSGetCurrentDisplayMode(did, &mut cur_mode_id);

        let mut raw_list = Vec::with_capacity(n_modes as usize);
        for idx in 0..n_modes {
            let mut buf = [0u8; 0xd8];
            if CGSGetDisplayModeDescriptionOfLength(did, idx, buf.as_mut_ptr(), 0xd8) != 0 {
                continue;
            }
            let mode_id = i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
            let width = u32::from_ne_bytes([buf[0x08], buf[0x09], buf[0x0a], buf[0x0b]]);
            let height = u32::from_ne_bytes([buf[0x0c], buf[0x0d], buf[0x0e], buf[0x0f]]);
            let depth = u32::from_ne_bytes([buf[0x10], buf[0x11], buf[0x12], buf[0x13]]);
            let hz_int = u32::from_ne_bytes([buf[0x24], buf[0x25], buf[0x26], buf[0x27]]);
            let pixel_width = u32::from_ne_bytes([buf[0xc8], buf[0xc9], buf[0xca], buf[0xcb]]);
            let pixel_height = u32::from_ne_bytes([buf[0xcc], buf[0xcd], buf[0xce], buf[0xcf]]);
            let scale = f32::from_ne_bytes([buf[0xd0], buf[0xd1], buf[0xd2], buf[0xd3]]);

            if width < 1440 || height < 800 {
                continue;
            }
            let hz = if hz_int > 0 && hz_int <= 600 {
                hz_int
            } else {
                60
            };
            let hidpi = scale > 1.5 || pixel_width > width;
            raw_list.push(RawCgsMode {
                mode_id,
                width,
                height,
                pixel_width: pixel_width.max(width),
                pixel_height: pixel_height.max(height),
                depth,
                hz,
                hidpi,
                current: mode_id == cur_mode_id,
            });
        }
        raw_list
    }
}

pub fn list_display_modes(prefer_external: bool) -> Vec<DisplayModeOption> {
    let Some((did, is_builtin)) = select_target_cg_display(prefer_external) else {
        return Vec::new();
    };
    let raw_list = read_cgs_modes_for_display(did);
    if raw_list.is_empty() {
        return Vec::new();
    }

    let cur_opt = raw_list.iter().find(|m| m.current).cloned();
    let target_aspect = cur_opt
        .as_ref()
        .map(|m| m.width as f32 / m.height.max(1) as f32)
        .unwrap_or(if is_builtin { 1512.0 / 982.0 } else { 16.0 / 9.0 });
    let target_depth = cur_opt.as_ref().map(|m| m.depth).unwrap_or(4);
    let _max_native_w = raw_list.iter().map(|m| m.width).max().unwrap_or(2560);

    let mut buckets: std::collections::BTreeMap<(u32, u32, bool), RawCgsMode> =
        std::collections::BTreeMap::new();
    for m in raw_list {
        let aspect = m.width as f32 / m.height.max(1) as f32;
        if !m.current && (aspect - target_aspect).abs() > 0.25 {
            continue;
        }
        // Keep all desktop HiDPI modes (1440..=3840) and native 5K (5120x2880 / 5120x3200),
        // filtering out 8K 1x backing modes (7680x4800 / 7680x4320) that only exist for 2x scaling.
        if !m.hidpi && !m.current && (m.width < 5120 || m.width > 5120) {
            continue;
        }
        let key = (m.width, m.height, m.hidpi);
        match buckets.get(&key) {
            None => {
                buckets.insert(key, m);
            }
            Some(existing) => {
                if m.current {
                    buckets.insert(key, m);
                } else if !existing.current {
                    let existing_score =
                        ((existing.depth == target_depth) as u32) * 1000 + existing.hz.min(500);
                    let new_score =
                        ((m.depth == target_depth) as u32) * 1000 + m.hz.min(500);
                    if new_score > existing_score {
                        buckets.insert(key, m);
                    }
                }
            }
        }
    }

    let mut out: Vec<DisplayModeOption> = buckets
        .into_values()
        .map(|m| {
            let suffix = if m.hidpi { "HiDPI" } else { "原生" };
            DisplayModeOption {
                mode_id: m.mode_id,
                width: m.width,
                height: m.height,
                pixel_width: m.pixel_width,
                pixel_height: m.pixel_height,
                hz: m.hz,
                hidpi: m.hidpi,
                current: m.current,
                is_builtin,
                label: format!("{}×{} · {}Hz {}", m.width, m.height, m.hz, suffix),
            }
        })
        .collect();

    out.sort_by(|a, b| {
        b.hidpi
            .cmp(&a.hidpi)
            .then_with(|| a.width.cmp(&b.width))
            .then_with(|| a.height.cmp(&b.height))
            .then_with(|| b.hz.cmp(&a.hz))
    });

    if out.len() > 10 {
        let mut trimmed = Vec::with_capacity(10);
        for item in out {
            if trimmed.len() < 9 || item.current || !item.hidpi {
                trimmed.push(item);
            }
        }
        return trimmed;
    }
    out
}

pub fn apply_display_mode(
    mode_id: i32,
    width: u32,
    height: u32,
    hz: u32,
    hidpi: bool,
    prefer_external: bool,
) -> Result<DisplayModeOption> {
    let Some((did, is_builtin)) = select_target_cg_display(prefer_external) else {
        bail!("未找到可配置的目标显示器");
    };
    let modes = list_display_modes(prefer_external);
    let chosen = modes
        .iter()
        .find(|m| mode_id >= 0 && m.mode_id == mode_id)
        .or_else(|| {
            modes.iter().find(|m| {
                m.width == width
                    && m.height == height
                    && m.hidpi == hidpi
                    && (hz == 0 || m.hz == hz)
            })
        })
        .or_else(|| {
            modes
                .iter()
                .find(|m| m.width == width && m.height == height && m.hidpi == hidpi)
        })
        .or_else(|| {
            modes
                .iter()
                .find(|m| m.width == width && m.height == height && (hz == 0 || m.hz == hz))
        })
        .cloned()
        .or_else(|| {
            let raw_all = read_cgs_modes_for_display(did);
            raw_all
                .iter()
                .find(|m| mode_id >= 0 && m.mode_id == mode_id)
                .or_else(|| {
                    raw_all.iter().find(|m| {
                        m.width == width
                            && m.height == height
                            && m.hidpi == hidpi
                            && (hz == 0 || m.hz == hz)
                    })
                })
                .or_else(|| {
                    raw_all
                        .iter()
                        .find(|m| m.width == width && m.height == height && m.hidpi == hidpi)
                })
                .cloned()
                .map(|m| {
                    let suffix = if m.hidpi { "HiDPI" } else { "原生" };
                    DisplayModeOption {
                        mode_id: m.mode_id,
                        width: m.width,
                        height: m.height,
                        pixel_width: m.pixel_width,
                        pixel_height: m.pixel_height,
                        hz: m.hz,
                        hidpi: m.hidpi,
                        current: true,
                        is_builtin,
                        label: format!("{}×{} · {}Hz {}", m.width, m.height, m.hz, suffix),
                    }
                })
        })
        .ok_or_else(|| anyhow::anyhow!("未找到匹配的显示模式 {}×{}", width, height))?;

    unsafe {
        let mut config: *mut c_void = std::ptr::null_mut();
        let begin_kr = CGBeginDisplayConfiguration(&mut config);
        ensure!(begin_kr == 0 && !config.is_null(), "CGBeginDisplayConfiguration 失败: {begin_kr}");
        let cfg_kr = CGSConfigureDisplayMode(config, did, chosen.mode_id);
        if cfg_kr != 0 {
            let _ = CGCancelDisplayConfiguration(config);
            bail!("CGSConfigureDisplayMode 切换分辨率失败: {cfg_kr}");
        }
        // 2 = kCGConfigurePermanently (same flag used by BetterDisplay)
        let done_kr = CGCompleteDisplayConfiguration(config, 2);
        ensure!(done_kr == 0, "CGCompleteDisplayConfiguration 提交失败: {done_kr}");
    }

    let mut updated = chosen;
    updated.current = true;
    Ok(updated)
}

pub fn apply_software_gamma(
    brightness_pct: u8,
    color_preset: u8,
    prefer_external: bool,
) -> Result<()> {
    let Some((did, is_builtin)) = select_target_cg_display(prefer_external) else {
        return Ok(());
    };

    if is_builtin {
        if let Some((_get_fn, set_fn)) = load_display_services() {
            let hw = (brightness_pct.clamp(5, 100) as f32) / 100.0;
            unsafe {
                let _ = set_fn(did, hw);
            }
        }
    }

    let (r_gain, g_gain, b_gain): (f32, f32, f32) = match color_preset {
        4 => (1.00, 0.91, 0.78), // 5000K 暖光
        6 => (0.92, 0.96, 1.00), // 7500K 清冷
        8 => (0.85, 0.92, 1.00), // 9300K 冷白
        _ => (1.00, 1.00, 1.00), // 6500K 标准 / 原生色温
    };

    // For built-in display, hardware backlight already handles 15%..100%; only apply software
    // gamma dimming when below 15% (sub-zero dimming) or when color temperature filter is active.
    let soft_bri = if is_builtin {
        if brightness_pct < 15 {
            (brightness_pct.max(3) as f32) / 15.0
        } else {
            1.0
        }
    } else {
        (brightness_pct.clamp(10, 100) as f32) / 100.0
    };

    if (soft_bri - 1.0).abs() < 0.01
        && (r_gain - 1.0).abs() < 0.01
        && (g_gain - 1.0).abs() < 0.01
        && (b_gain - 1.0).abs() < 0.01
    {
        unsafe {
            CGDisplayRestoreColorSyncSettings();
        }
        return Ok(());
    }

    let mut r_tbl = [0.0f32; 256];
    let mut g_tbl = [0.0f32; 256];
    let mut b_tbl = [0.0f32; 256];
    for i in 0..256 {
        let base = (i as f32) / 255.0;
        r_tbl[i] = (base * soft_bri * r_gain).clamp(0.0, 1.0);
        g_tbl[i] = (base * soft_bri * g_gain).clamp(0.0, 1.0);
        b_tbl[i] = (base * soft_bri * b_gain).clamp(0.0, 1.0);
    }

    unsafe {
        let _ = CGSetDisplayTransferByTable(
            did,
            256,
            r_tbl.as_ptr(),
            g_tbl.as_ptr(),
            b_tbl.as_ptr(),
        );
    }
    Ok(())
}

static BUILTIN_DISPLAY_CONFINED: AtomicBool = AtomicBool::new(false);

fn find_builtin_display_id() -> Option<CGDirectDisplayID> {
    unsafe {
        let mut ids = [0 as CGDirectDisplayID; 16];
        let mut count: u32 = 0;
        let rc = CGGetOnlineDisplayList(16, ids.as_mut_ptr(), &mut count);
        if rc != 0 || count == 0 {
            return None;
        }
        ids[..(count as usize).min(16)]
            .iter()
            .copied()
            .find(|&did| CGDisplayIsBuiltin(did) != 0)
    }
}

pub fn set_builtin_display_confined(confined: bool) {
    BUILTIN_DISPLAY_CONFINED.store(confined, Ordering::Relaxed);
}

pub fn clamp_cursor_to_builtin_if_needed() -> bool {
    unsafe {
        let Some(builtin_id) = find_builtin_display_id() else {
            return false;
        };
        let b = CGDisplayBounds(builtin_id);
        if b.size.width <= 4.0 || b.size.height <= 4.0 {
            return false;
        }
        let ev = CGEventCreate(std::ptr::null());
        if ev.is_null() {
            return false;
        }
        let pt = CGEventGetLocation(ev);
        CFRelease(ev as CFTypeRef);

        let min_x = b.origin.x;
        let max_x = b.origin.x + b.size.width - 1.0;
        let min_y = b.origin.y;
        let max_y = b.origin.y + b.size.height - 1.0;
        if pt.x >= min_x && pt.x <= max_x && pt.y >= min_y && pt.y <= max_y {
            return false;
        }

        let target = CGPoint {
            x: pt.x.clamp(min_x + 2.0, max_x - 2.0),
            y: pt.y.clamp(min_y + 2.0, max_y - 2.0),
        };
        CGWarpMouseCursorPosition(target) == 0
    }
}

pub fn sample_pointer_and_ctrl() -> Option<PointerSample> {
    unsafe {
        let ev = CGEventCreate(std::ptr::null());
        if ev.is_null() {
            return None;
        }
        let pt = CGEventGetLocation(ev);
        CFRelease(ev as CFTypeRef);

        let confined_builtin = BUILTIN_DISPLAY_CONFINED
            .load(Ordering::Relaxed)
            .then(find_builtin_display_id)
            .flatten();

        let target_disp = if let Some(bid) = confined_builtin {
            bid
        } else {
            let mut disp_id: CGDirectDisplayID = 0;
            let mut count: u32 = 0;
            let rc = CGGetDisplaysWithPoint(pt, 1, &mut disp_id, &mut count);
            if rc == 0 && count > 0 {
                disp_id
            } else {
                CGMainDisplayID()
            }
        };
        let bounds = CGDisplayBounds(target_disp);
        let ctrl_down = CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x3B)
            || CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x3E);
        let mouse_down = CGEventSourceButtonState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0)
            || CGEventSourceButtonState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 1)
            || CGEventSourceButtonState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 2);

        let left = bounds.origin.x.round() as i32;
        let top = bounds.origin.y.round() as i32;
        let right = (bounds.origin.x + bounds.size.width).round() as i32;
        let bottom = (bounds.origin.y + bounds.size.height).round() as i32;

        Some(PointerSample {
            x: pt.x.round() as i32,
            y: pt.y.round() as i32,
            screen_left: left,
            screen_top: top,
            screen_right: right,
            screen_bottom: bottom,
            ctrl_down,
            mouse_down,
        })
    }
}

pub fn is_outer_desktop_edge(x: i32, y: i32, side: EdgeSide) -> bool {
    let probe_x = match side {
        EdgeSide::Left => (x - 16) as f64,
        EdgeSide::Right => (x + 16) as f64,
    };
    if BUILTIN_DISPLAY_CONFINED.load(Ordering::Relaxed) {
        if let Some(bid) = find_builtin_display_id() {
            let b = unsafe { CGDisplayBounds(bid) };
            let min_x = b.origin.x;
            let max_x = b.origin.x + b.size.width;
            return probe_x < min_x || probe_x >= max_x;
        }
    }
    let pt = CGPoint {
        x: probe_x,
        y: y as f64,
    };
    unsafe {
        let mut disp_id: CGDirectDisplayID = 0;
        let mut count: u32 = 0;
        let rc = CGGetDisplaysWithPoint(pt, 1, &mut disp_id, &mut count);
        !(rc == 0 && count > 0)
    }
}

pub fn raw_hotkey_slot_mask() -> u8 {
    unsafe {
        let ctrl = CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x3B)
            || CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x3E);
        let opt = CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x3A)
            || CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x3D);
        if !ctrl || !opt {
            return 0;
        }
        let mut mask = 0u8;
        if CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x12) {
            mask |= 0b001;
        }
        if CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x13) {
            mask |= 0b010;
        }
        if CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x14) {
            mask |= 0b100;
        }
        mask
    }
}

pub fn raw_media_adjust_mask() -> u8 {
    unsafe {
        let ctrl = CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x3B)
            || CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x3E);
        let opt = CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x3A)
            || CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x3D);
        if !ctrl || !opt {
            return 0;
        }
        let mut mask = 0u8;
        // 0x7E = UpArrow, 0x7D = DownArrow, 0x7C = RightArrow, 0x7B = LeftArrow, 0x2E = M
        if CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x7E) {
            mask |= 0b00001;
        }
        if CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x7D) {
            mask |= 0b00010;
        }
        if CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x7C) {
            mask |= 0b00100;
        }
        if CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x7B) {
            mask |= 0b01000;
        }
        if CGEventSourceKeyState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE, 0x2E) {
            mask |= 0b10000;
        }
        mask
    }
}

pub fn trigger_internal_deskflow_slot(slot: u8) -> Result<()> {
    let keycode: u16 = match slot {
        1 => 0x4F, // kVK_F18
        2 => 0x50, // kVK_F19
        3 => 0x5A, // kVK_F20
        _ => bail!("不支持的内部切屏槽位: {slot}"),
    };
    unsafe {
        let down = CGEventCreateKeyboardEvent(std::ptr::null(), keycode, true);
        ensure!(!down.is_null(), "创建键盘按下事件失败");
        CGEventSetFlags(down, 0);
        CGEventPost(K_CG_HID_EVENT_TAP, down);
        CFRelease(down as CFTypeRef);

        thread::sleep(Duration::from_millis(15));

        let up = CGEventCreateKeyboardEvent(std::ptr::null(), keycode, false);
        ensure!(!up.is_null(), "创建键盘抬起事件失败");
        CGEventSetFlags(up, 0);
        CGEventPost(K_CG_HID_EVENT_TAP, up);
        CFRelease(up as CFTypeRef);
    }
    Ok(())
}

/// Strictly detects when THIS Mac is the controlled target (`被控端`) of an active incoming
/// remote desktop session (`UURemoteServer` / `screensharingd`).
/// Outgoing client sessions (`UURemote` main app controlling an office PC, which only holds
/// `"UURemote Disable Idle System Sleep"`) are intentionally NOT matched so home 3-machine
/// seamless switching remains 100% active while remoting into an external PC.
pub fn is_remote_session_active() -> bool {
    let now = std::time::Instant::now();
    if let Ok(guard) = REMOTE_SESSION_CACHE.lock() {
        if let Some((ts, cached)) = *guard {
            if now.duration_since(ts) < Duration::from_millis(2400) {
                return cached;
            }
        }
    }

    let active = Command::new("/usr/bin/pmset")
        .args(["-g", "assertions"])
        .output()
        .ok()
        .map(|out| {
            let text = String::from_utf8_lossy(&out.stdout);
            text.contains("UURemote Disable Display Sleep")
                || text.contains("screensharingd")
                || text.contains("RFBRegisterMDNSService")
        })
        .unwrap_or(false);

    if let Ok(mut guard) = REMOTE_SESSION_CACHE.lock() {
        *guard = Some((now, active));
    }
    active
}

pub fn is_edge_locked() -> bool {
    is_remote_session_active()
}

pub fn kill_orphan_deskflow_processes() {
    let _ = Command::new("/usr/bin/pkill")
        .args(["-9", "-x", "deskflow-core"])
        .status();
}

pub fn is_secure_input_enabled() -> bool {
    unsafe { IsSecureEventInputEnabled() != 0 }
}

pub fn is_clamshell_closed() -> bool {
    unsafe {
        let matching = IOServiceMatching(b"IOPMrootDomain\0".as_ptr() as *const c_char);
        if matching.is_null() {
            return false;
        }
        let entry = IOServiceGetMatchingService(0, matching);
        if entry == 0 {
            return false;
        }
        let Ok(cf_key) = make_cfstring(b"AppleClamshellState\0") else {
            IOObjectRelease(entry);
            return false;
        };
        let prop = IORegistryEntryCreateCFProperty(entry, cf_key.0, std::ptr::null(), 0);
        IOObjectRelease(entry);
        if prop.is_null() {
            return false;
        }
        let closed = if CFGetTypeID(prop) == CFBooleanGetTypeID() {
            CFBooleanGetValue(prop)
        } else {
            false
        };
        CFRelease(prop);
        closed
    }
}

pub fn gather_external_windows_to_builtin() -> Result<()> {
    let script = r#"
        tell application "System Events"
            repeat with proc in (every process whose background only is false)
                try
                    repeat with win in (every window of proc)
                        set pos to position of win
                        if (item 1 of pos) < 0 or (item 2 of pos) < 0 then
                            set position of win to {80, 80}
                        end if
                    end repeat
                end try
            end repeat
        end tell
    "#;
    let _ = Command::new("/usr/bin/osascript")
        .args(["-e", script])
        .status();
    Ok(())
}

static LAST_PASTEBOARD_CHANGE_COUNT: AtomicIsize = AtomicIsize::new(-1);

pub fn read_clipboard_png() -> Option<Vec<u8>> {
    unsafe {
        type Msg0 = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
        type Msg1 = unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void) -> *mut c_void;
        type MsgCount = unsafe extern "C" fn(*mut c_void, *mut c_void) -> isize;
        type MsgLen = unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize;
        type MsgRepPng = unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            usize,
            *mut c_void,
        ) -> *mut c_void;

        let send0: Msg0 = std::mem::transmute(objc_msgSend as *const ());
        let send1: Msg1 = std::mem::transmute(objc_msgSend as *const ());
        let send_count: MsgCount = std::mem::transmute(objc_msgSend as *const ());
        let send_len: MsgLen = std::mem::transmute(objc_msgSend as *const ());
        let send_rep_png: MsgRepPng = std::mem::transmute(objc_msgSend as *const ());

        let cls_pb = objc_getClass(b"NSPasteboard\0".as_ptr() as *const c_char);
        if cls_pb.is_null() {
            return None;
        }
        let sel_gen = sel_registerName(b"generalPasteboard\0".as_ptr() as *const c_char);
        let pb = send0(cls_pb, sel_gen);
        if pb.is_null() {
            return None;
        }

        let sel_count = sel_registerName(b"changeCount\0".as_ptr() as *const c_char);
        let cur_count = send_count(pb, sel_count);
        if cur_count == LAST_PASTEBOARD_CHANGE_COUNT.load(Ordering::Relaxed) {
            return None;
        }
        LAST_PASTEBOARD_CHANGE_COUNT.store(cur_count, Ordering::Relaxed);

        let sel_data_for = sel_registerName(b"dataForType:\0".as_ptr() as *const c_char);
        let sel_bytes = sel_registerName(b"bytes\0".as_ptr() as *const c_char);
        let sel_length = sel_registerName(b"length\0".as_ptr() as *const c_char);

        let cf_png = make_cfstring(b"public.png\0").ok()?;
        let mut ns_data = send1(pb, sel_data_for, cf_png.0);

        // Fallback: if clipboard holds public.tiff (e.g. certain macOS screenshot tools), convert to PNG
        if ns_data.is_null() {
            let cf_tiff = make_cfstring(b"public.tiff\0").ok()?;
            let tiff_data = send1(pb, sel_data_for, cf_tiff.0);
            if !tiff_data.is_null() {
                let cls_rep = objc_getClass(b"NSBitmapImageRep\0".as_ptr() as *const c_char);
                if !cls_rep.is_null() {
                    let sel_rep =
                        sel_registerName(b"imageRepWithData:\0".as_ptr() as *const c_char);
                    let rep = send1(cls_rep, sel_rep, tiff_data);
                    if !rep.is_null() {
                        let sel_repr = sel_registerName(
                            b"representationUsingType:properties:\0".as_ptr() as *const c_char,
                        );
                        // NSBitmapImageFileTypePNG = 4
                        ns_data = send_rep_png(rep, sel_repr, 4, std::ptr::null_mut());
                    }
                }
            }
        }

        if ns_data.is_null() {
            return None;
        }
        let len = send_len(ns_data, sel_length);
        if len == 0 || len > 32 * 1024 * 1024 {
            return None;
        }
        let ptr = send0(ns_data, sel_bytes) as *const u8;
        if ptr.is_null() {
            return None;
        }
        Some(std::slice::from_raw_parts(ptr, len).to_vec())
    }
}

pub fn write_clipboard_png(png_bytes: &[u8]) -> Result<()> {
    unsafe {
        type Msg0 = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
        type MsgClear = unsafe extern "C" fn(*mut c_void, *mut c_void) -> isize;
        type MsgDataWith =
            unsafe extern "C" fn(*mut c_void, *mut c_void, *const u8, usize) -> *mut c_void;
        type MsgSetData =
            unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *const c_void) -> u8;

        let send0: Msg0 = std::mem::transmute(objc_msgSend as *const ());
        let send_clear: MsgClear = std::mem::transmute(objc_msgSend as *const ());
        let send_data: MsgDataWith = std::mem::transmute(objc_msgSend as *const ());
        let send_set: MsgSetData = std::mem::transmute(objc_msgSend as *const ());

        let cls_pb = objc_getClass(b"NSPasteboard\0".as_ptr() as *const c_char);
        let cls_data = objc_getClass(b"NSData\0".as_ptr() as *const c_char);
        ensure!(!cls_pb.is_null() && !cls_data.is_null(), "无法获取 NSPasteboard 类");

        let pb = send0(
            cls_pb,
            sel_registerName(b"generalPasteboard\0".as_ptr() as *const c_char),
        );
        ensure!(!pb.is_null(), "generalPasteboard 为空");

        let _ = send_clear(
            pb,
            sel_registerName(b"clearContents\0".as_ptr() as *const c_char),
        );
        let ns_data = send_data(
            cls_data,
            sel_registerName(b"dataWithBytes:length:\0".as_ptr() as *const c_char),
            png_bytes.as_ptr(),
            png_bytes.len(),
        );
        ensure!(!ns_data.is_null(), "构造 NSData 失败");

        let cf_png = make_cfstring(b"public.png\0")?;
        let ok = send_set(
            pb,
            sel_registerName(b"setData:forType:\0".as_ptr() as *const c_char),
            ns_data,
            cf_png.0,
        );
        ensure!(ok != 0, "写入 macOS 剪贴板 PNG 失败");
        let new_count = send_clear(
            pb,
            sel_registerName(b"changeCount\0".as_ptr() as *const c_char),
        );
        LAST_PASTEBOARD_CHANGE_COUNT.store(new_count, Ordering::Relaxed);
        Ok(())
    }
}

pub fn read_system_volume() -> Option<u8> {
    let out = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", "output volume of (get volume settings)"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.trim().parse::<u8>().ok().map(|v| v.min(100))
}

pub fn write_system_volume(volume: u8) -> Result<()> {
    let v = volume.min(100);
    let script = format!("set volume output volume {v} without output muted");
    let status = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .status()?;
    ensure!(status.success(), "osascript 设置系统音量失败");
    Ok(())
}

pub fn write_system_audio_mute(muted: bool) -> Result<()> {
    let script = if muted {
        "set volume with output muted"
    } else {
        "set volume without output muted"
    };
    let status = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", script])
        .status()?;
    ensure!(status.success(), "osascript 设置系统静音失败");
    Ok(())
}

pub fn kill_other_kvmddc_instances() {
    let my_pid = std::process::id();
    if let Ok(out) = std::process::Command::new("/usr/bin/pgrep")
        .args(["-x", "kvmddc"])
        .output()
    {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Ok(pid) = line.trim().parse::<u32>() {
                if pid != my_pid {
                    let _ = std::process::Command::new("/bin/kill")
                        .args(["-9", &pid.to_string()])
                        .status();
                }
            }
        }
    }
}

/// Returns how many CoreGraphics displays are currently awake & active (excludes asleep/blanked displays).
pub fn active_display_count() -> u32 {
    unsafe {
        let mut ids = [0u32; 16];
        let mut cnt = 0u32;
        if CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut cnt) == 0 {
            cnt
        } else {
            0
        }
    }
}

/// Declares local user activity so macOS wakes any display currently sitting in OS display sleep.
pub fn wake_idle_display_for_kvm() {
    let Ok(cf_name) = make_cfstring(b"Kvmddc Deskflow active display wake\0") else {
        return;
    };
    if let Ok(mut id) = USER_WAKE_ASSERTION_ID.lock() {
        unsafe {
            let _ = IOPMAssertionDeclareUserActivity(cf_name.0, 0, &mut *id);
        }
    }
}

/// Holds or releases `PreventUserIdleDisplaySleep` so macOS never drops `CGGetActiveDisplayList`
/// to 0 on headless / switched-away Macs (which otherwise crashes `deskflow-core` with
/// "failed to initialize computer shape").
pub fn ensure_display_sleep_prevented(enabled: bool) {
    let Ok(mut guard) = DISPLAY_AWAKE_ASSERTION_ID.lock() else {
        return;
    };
    if enabled {
        if guard.is_none() {
            if let (Ok(cf_type), Ok(cf_name)) = (
                make_cfstring(b"PreventUserIdleDisplaySleep\0"),
                make_cfstring(b"Kvmddc keep Deskflow display awake\0"),
            ) {
                let mut assertion_id = 0u32;
                let kr = unsafe {
                    IOPMAssertionCreateWithName(
                        cf_type.0,
                        255, // kIOPMAssertionLevelOn
                        cf_name.0,
                        &mut assertion_id,
                    )
                };
                if kr == 0 && assertion_id != 0 {
                    *guard = Some(assertion_id);
                }
            }
        }
        drop(guard);
        if active_display_count() == 0 {
            wake_idle_display_for_kvm();
        }
    } else if let Some(assertion_id) = guard.take() {
        unsafe {
            let _ = IOPMAssertionRelease(assertion_id);
        }
    }
}

unsafe fn io_registry_bool_prop(entry: IoRegistryEntryT, key: &[u8]) -> Option<bool> {
    let cf_key = make_cfstring(key).ok()?;
    let prop = IORegistryEntryCreateCFProperty(entry, cf_key.0, std::ptr::null(), 0);
    if prop.is_null() {
        return None;
    }
    let res = if CFGetTypeID(prop) == CFBooleanGetTypeID() {
        Some(CFBooleanGetValue(prop))
    } else {
        None
    };
    CFRelease(prop);
    res
}

unsafe fn io_registry_i64_prop(entry: IoRegistryEntryT, key: &[u8]) -> Option<i64> {
    let cf_key = make_cfstring(key).ok()?;
    let prop = IORegistryEntryCreateCFProperty(entry, cf_key.0, std::ptr::null(), 0);
    if prop.is_null() {
        return None;
    }
    let res = cf_number_to_i64(prop);
    CFRelease(prop);
    res
}

unsafe fn io_registry_string_prop(entry: IoRegistryEntryT, key: &[u8]) -> Option<String> {
    let cf_key = make_cfstring(key).ok()?;
    let prop = IORegistryEntryCreateCFProperty(entry, cf_key.0, std::ptr::null(), 0);
    if prop.is_null() {
        return None;
    }
    let res = cf_string_to_rust(prop);
    CFRelease(prop);
    res
}

pub fn list_external_input_devices() -> Vec<crate::ExternalInputDevice> {
    let mut devices = Vec::new();
    let mut seen = std::collections::HashSet::new();
    unsafe {
        let matching = IOServiceMatching(b"IOHIDDevice\0".as_ptr() as *const c_char);
        if matching.is_null() {
            return devices;
        }
        let mut iter: IoIteratorT = 0;
        let kr = IOServiceGetMatchingServices(0, matching, &mut iter);
        if kr != 0 || iter == 0 {
            return devices;
        }
        loop {
            let entry = IOIteratorNext(iter);
            if entry == 0 {
                break;
            }
            let built_in = io_registry_bool_prop(entry, b"Built-In\0");
            let transport = io_registry_string_prop(entry, b"Transport\0");
            let usage_page = io_registry_i64_prop(entry, b"PrimaryUsagePage\0");
            let usage = io_registry_i64_prop(entry, b"PrimaryUsage\0");
            let vendor_id = io_registry_i64_prop(entry, b"VendorID\0");
            let product_id = io_registry_i64_prop(entry, b"ProductID\0");
            let location_id = io_registry_i64_prop(entry, b"LocationID\0");
            let product = io_registry_string_prop(entry, b"Product\0");
            if let Some(dev) = crate::classify_macos_hid_device(
                built_in,
                transport.as_deref(),
                usage_page,
                usage,
                vendor_id,
                product_id,
                location_id,
                product.as_deref(),
            ) {
                if seen.insert(dev.device_key.clone()) {
                    devices.push(dev);
                }
            }
            IOObjectRelease(entry);
        }
        IOObjectRelease(iter);
    }
    devices.sort();
    devices
}

#[repr(C)]
struct DarwinIfAddrs {
    ifa_next: *mut DarwinIfAddrs,
    ifa_name: *const c_char,
    ifa_flags: u32,
    ifa_addr: *mut DarwinSockAddr,
    ifa_netmask: *mut DarwinSockAddr,
    ifa_dstaddr: *mut DarwinSockAddr,
    ifa_data: *mut c_void,
}

#[repr(C)]
struct DarwinSockAddr {
    sa_len: u8,
    sa_family: u8,
    sa_data: [u8; 14],
}

#[repr(C)]
struct DarwinSockAddrIn {
    sin_len: u8,
    sin_family: u8,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

extern "C" {
    fn getifaddrs(ifap: *mut *mut DarwinIfAddrs) -> i32;
    fn freeifaddrs(ifa: *mut DarwinIfAddrs);
}

/// Returns active non-loopback IPv4 addresses on macOS interfaces (`getifaddrs`),
/// ordering persistent built-in interfaces (`en0`, `en1`) ahead of external USB/dock NICs.
pub fn list_local_ipv4_addrs() -> Vec<String> {
    const IFF_UP: u32 = 0x1;
    const IFF_LOOPBACK: u32 = 0x8;
    const AF_INET: u8 = 2;

    let mut ranked: Vec<(u8, usize, String)> = Vec::new();
    let mut head: *mut DarwinIfAddrs = std::ptr::null_mut();
    unsafe {
        if getifaddrs(&mut head) != 0 || head.is_null() {
            return Vec::new();
        }
        let mut cur = head;
        let mut idx = 0usize;
        while !cur.is_null() {
            let ifa = &*cur;
            if (ifa.ifa_flags & IFF_UP) != 0
                && (ifa.ifa_flags & IFF_LOOPBACK) == 0
                && !ifa.ifa_addr.is_null()
                && (*ifa.ifa_addr).sa_family == AF_INET
            {
                let if_name = if ifa.ifa_name.is_null() {
                    ""
                } else {
                    std::ffi::CStr::from_ptr(ifa.ifa_name).to_str().unwrap_or("")
                };
                let prio = match if_name {
                    "en0" => 0,
                    "en1" => 1,
                    _ => 2,
                };
                let sin = &*(ifa.ifa_addr as *const DarwinSockAddrIn);
                let ip = std::net::Ipv4Addr::from(sin.sin_addr);
                if !ip.is_unspecified() && !ip.is_loopback() {
                    let s = ip.to_string();
                    if !ranked.iter().any(|(_, _, existing)| existing == &s) {
                        ranked.push((prio, idx, s));
                        idx += 1;
                    }
                }
            }
            cur = ifa.ifa_next;
        }
        freeifaddrs(head);
    }
    ranked.sort_by_key(|(prio, idx, _)| (*prio, *idx));
    ranked.into_iter().map(|(_, _, ip)| ip).collect()
}
