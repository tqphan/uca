// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Monitors and DPI (05, 07). The process must be Per-Monitor v2 DPI aware
//! for physical coordinates and scale factors to be right.

use uca_types::{MonitorId, MonitorInfo, RectPhys};
use windows_sys::Win32::Foundation::{LPARAM, RECT, TRUE};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, DEVMODEW, ENUM_CURRENT_SETTINGS,
    HDC, HMONITOR, MONITORINFOEXW,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, MONITORINFOF_PRIMARY, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

fn rect(r: RECT) -> RectPhys {
    RectPhys::new(
        f64::from(r.left),
        f64::from(r.top),
        f64::from(r.right - r.left),
        f64::from(r.bottom - r.top),
    )
}

unsafe extern "system" fn collect(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> i32 {
    // SAFETY: `data` is the Vec passed by `monitors()` for the duration of the call.
    let list = unsafe { &mut *(data as *mut Vec<HMONITOR>) };
    list.push(m);
    TRUE
}

fn wide_to_string(w: &[u16]) -> String {
    let end = w.iter().position(|c| *c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

pub fn monitors() -> Vec<MonitorInfo> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: the callback only pushes into `handles`, which outlives the call.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            &mut handles as *mut _ as LPARAM,
        );
    }
    handles
        .into_iter()
        .enumerate()
        .filter_map(|(i, h)| {
            // SAFETY: MONITORINFOEXW is plain data; cbSize is set before the call.
            let mut info: MONITORINFOEXW = unsafe { std::mem::zeroed() };
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            // SAFETY: `h` comes from EnumDisplayMonitors; `info` is sized correctly.
            if unsafe { GetMonitorInfoW(h, &mut info as *mut _ as *mut _) } == 0 {
                return None;
            }
            let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
            // SAFETY: out-pointers to locals.
            let dpi_ok =
                unsafe { GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) } == 0;
            let scale = if dpi_ok { f64::from(dpi_x) / 96.0 } else { 1.0 };
            // SAFETY: DEVMODEW is plain data; dmSize is set before the call.
            let mut mode: DEVMODEW = unsafe { std::mem::zeroed() };
            mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
            // SAFETY: the device name is NUL-terminated by GetMonitorInfoW.
            let refresh = if unsafe {
                EnumDisplaySettingsW(info.szDevice.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode)
            } != 0
            {
                mode.dmDisplayFrequency as f32
            } else {
                0.0
            };
            Some(MonitorInfo {
                id: MonitorId(i as u32),
                name: wide_to_string(&info.szDevice),
                primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                bounds_phys: rect(info.monitorInfo.rcMonitor),
                work_area_phys: rect(crate::overlay::without_own_strips(
                    info.monitorInfo.rcMonitor,
                    info.monitorInfo.rcWork,
                )),
                scale_factor: scale,
                refresh_hz: refresh,
            })
        })
        .collect()
}

/// The virtual desktop: the bounding box of all monitors.
pub fn virtual_desktop() -> RectPhys {
    // SAFETY: plain metric queries.
    unsafe {
        RectPhys::new(
            f64::from(GetSystemMetrics(SM_XVIRTUALSCREEN)),
            f64::from(GetSystemMetrics(SM_YVIRTUALSCREEN)),
            f64::from(GetSystemMetrics(SM_CXVIRTUALSCREEN)),
            f64::from(GetSystemMetrics(SM_CYVIRTUALSCREEN)),
        )
    }
}
