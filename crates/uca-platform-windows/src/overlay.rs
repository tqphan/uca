// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Board overlay windows (05, 07): topmost, never activated, not in the taskbar
//! or Alt+Tab, optional click-through, and AppBar space reservation.

use std::sync::Mutex;

use uca_platform::{Edge, OverlayOptions, PlatformError, RawWindowHandle, RectPhys, Result};
use uca_types::PhysPoint;
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows_sys::Win32::Graphics::Gdi::PtInRect;
use windows_sys::Win32::UI::Shell::{
    SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_NEW, ABM_REMOVE, ABM_SETPOS,
    APPBARDATA,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindow, GetWindowLongPtrW, GetWindowRect, IsWindowVisible, SetLayeredWindowAttributes,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, GW_HWNDPREV, HWND_TOPMOST, LWA_ALPHA,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE,
    WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT,
};

fn hwnd(w: RawWindowHandle) -> Result<HWND> {
    match w {
        RawWindowHandle::Win32(h) => Ok(h.hwnd.get() as HWND),
        _ => Err(PlatformError::Unsupported("not a Win32 window")),
    }
}

fn update_ex_style(h: HWND, set: u32, clear: u32) {
    // SAFETY: style changes on a window handle owned by this process.
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        let new = (ex | set) & !clear;
        if new != ex {
            SetWindowLongPtrW(h, GWL_EXSTYLE, new as isize);
        }
    }
}

/// UCA's overlay windows, for [`covered_at`].
static OVERLAYS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
/// UCA's ordinary windows whose widgets the engine hit-tests (the Settings window, 05).
static TRACKED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

fn ex_style(h: HWND) -> u32 {
    // SAFETY: reading a style of a window handle; a stale handle returns 0.
    unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 }
}

fn window_rect(h: HWND) -> Option<RECT> {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: out-pointer to a local.
    (unsafe { GetWindowRect(h, &mut r) } != 0).then_some(r)
}

/// Shown on screen: visible and not cloaked by DWM (hidden UWP windows are).
fn on_screen(h: HWND) -> bool {
    let mut cloaked: u32 = 0;
    // SAFETY: plain queries; the out-pointer is a local of the size passed.
    unsafe {
        IsWindowVisible(h) != 0
            && (DwmGetWindowAttribute(
                h,
                DWMWA_CLOAKED as u32,
                (&mut cloaked as *mut u32).cast(),
                std::mem::size_of::<u32>() as u32,
            ) != 0
                || cloaked == 0)
    }
}

fn contains(h: HWND, pt: POINT) -> bool {
    // SAFETY: point test on a local rect.
    window_rect(h).is_some_and(|r| unsafe { PtInRect(&r, pt) } != 0)
}

/// Whether a window that isn't one of `own` lies above `h` at `pt`.
fn covered_above(h: HWND, pt: POINT, own: &[usize]) -> bool {
    // Walk the windows above `h` in z-order.
    // SAFETY: z-order queries; a null handle ends the walk.
    let mut w = unsafe { GetWindow(h, GW_HWNDPREV) };
    while !w.is_null() {
        let counts = !own.contains(&(w as usize))
            && ex_style(w) & WS_EX_TRANSPARENT == 0
            && on_screen(w)
            && contains(w, pt);
        if counts {
            return true;
        }
        // SAFETY: as above.
        w = unsafe { GetWindow(w, GW_HWNDPREV) };
    }
    false
}

/// On an overlay: whether another window lies above it. Elsewhere, while any
/// window is tracked: whether the point misses every tracked window that is
/// on screen there and not covered (so a minimised one counts as covered).
pub fn covered_at(p: PhysPoint) -> bool {
    let pt = POINT {
        x: p.x.round() as i32,
        y: p.y.round() as i32,
    };
    let overlays = OVERLAYS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let tracked = TRACKED.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let under = |h: HWND| on_screen(h) && contains(h, pt);
    let on_overlays: Vec<HWND> = overlays
        .iter()
        .map(|&o| o as HWND)
        .filter(|&o| under(o))
        .collect();
    if !on_overlays.is_empty() {
        return on_overlays.iter().any(|&o| covered_above(o, pt, &overlays));
    }
    if tracked.is_empty() {
        return false;
    }
    let own: Vec<usize> = overlays.iter().chain(&tracked).copied().collect();
    !tracked
        .iter()
        .map(|&t| t as HWND)
        .any(|t| under(t) && !covered_above(t, pt, &own))
}

/// Adds `w` to, or removes it from, the windows [`covered_at`] tracks.
pub fn track_covered(w: RawWindowHandle, track: bool) -> Result<()> {
    let h = hwnd(w)? as usize;
    let mut tracked = TRACKED.lock().unwrap_or_else(|e| e.into_inner());
    tracked.retain(|&t| t != h);
    if track {
        tracked.push(h);
    }
    Ok(())
}

pub fn configure(w: RawWindowHandle, opts: OverlayOptions) -> Result<()> {
    let h = hwnd(w)?;
    {
        let mut overlays = OVERLAYS.lock().unwrap_or_else(|e| e.into_inner());
        overlays.retain(|&o| o != h as usize);
        if !opts.display_only {
            overlays.push(h as usize);
        }
    }
    let mut set = WS_EX_TOOLWINDOW;
    if opts.no_focus {
        set |= WS_EX_NOACTIVATE;
    }
    if opts.topmost {
        set |= WS_EX_TOPMOST;
    }
    update_ex_style(h, set, WS_EX_APPWINDOW);
    if opts.topmost {
        // SAFETY: reorders our own window without moving, sizing or activating it.
        unsafe {
            SetWindowPos(
                h,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }
    Ok(())
}

pub fn show(w: RawWindowHandle, visible: bool) -> Result<()> {
    let h = hwnd(w)?;
    // SAFETY: shows or hides our own window; SW_SHOWNOACTIVATE never takes focus.
    unsafe { ShowWindow(h, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE }) };
    Ok(())
}

pub fn click_through(w: RawWindowHandle, through: bool) -> Result<()> {
    let h = hwnd(w)?;
    if through {
        update_ex_style(h, WS_EX_LAYERED | WS_EX_TRANSPARENT, 0);
        // A layered window needs attributes to stay visible.
        // SAFETY: fully opaque alpha on our own window.
        unsafe { SetLayeredWindowAttributes(h, 0, 255, LWA_ALPHA) };
    } else {
        update_ex_style(h, 0, WS_EX_TRANSPARENT);
    }
    Ok(())
}

/// UCA's own reserved strips, by window, so the reported work area can leave
/// them out (02 `area`): a board laid out in the work area must not push
/// itself away from the edge.
static OWN_STRIPS: Mutex<Vec<(usize, Edge, RECT)>> = Mutex::new(Vec::new());

fn set_own_strip(h: HWND, strip: Option<(Edge, RECT)>) {
    let mut own = OWN_STRIPS.lock().unwrap_or_else(|e| e.into_inner());
    own.retain(|(w, ..)| *w != h as usize);
    if let Some((edge, r)) = strip {
        own.push((h as usize, edge, r));
    }
}

/// `work` with UCA's own strips on `monitor` added back.
pub fn without_own_strips(monitor: RECT, mut work: RECT) -> RECT {
    let own = OWN_STRIPS.lock().unwrap_or_else(|e| e.into_inner());
    for (_, edge, r) in own.iter() {
        let on_monitor = r.left < monitor.right
            && r.right > monitor.left
            && r.top < monitor.bottom
            && r.bottom > monitor.top;
        if !on_monitor {
            continue;
        }
        // The shell moved the work area's edge to the strip's inner side.
        match edge {
            Edge::Bottom if (work.bottom - r.top).abs() <= 1 => {
                work.bottom = work.bottom.max(r.bottom)
            }
            Edge::Top if (work.top - r.bottom).abs() <= 1 => work.top = work.top.min(r.top),
            Edge::Left if (work.left - r.right).abs() <= 1 => work.left = work.left.min(r.left),
            Edge::Right if (work.right - r.left).abs() <= 1 => work.right = work.right.max(r.right),
            _ => {}
        }
    }
    work
}

fn rect(r: RectPhys) -> RECT {
    RECT {
        left: r.x.round() as i32,
        top: r.y.round() as i32,
        right: (r.x + r.width).round() as i32,
        bottom: (r.y + r.height).round() as i32,
    }
}

fn appbar(h: HWND) -> APPBARDATA {
    // SAFETY: APPBARDATA is plain data; cbSize and hWnd are set before use.
    let mut d: APPBARDATA = unsafe { std::mem::zeroed() };
    d.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
    d.hWnd = h;
    d
}

pub fn reserve_edge(w: RawWindowHandle, edge: Edge, monitor: RectPhys, size: u32) -> Result<()> {
    let h = hwnd(w)?;
    let mut d = appbar(h);
    // SAFETY: registering our own window as an app bar; repeated ABM_NEW is harmless.
    unsafe { SHAppBarMessage(ABM_NEW, &mut d) };
    let size = size as i32;
    // The strip on the full bounds; the shell's adjusted position is not used and
    // the window is not moved (02, decided), so drawing and hit-testing match.
    let mut r = rect(monitor);
    d.uEdge = match edge {
        Edge::Top => ABE_TOP,
        Edge::Bottom => ABE_BOTTOM,
        Edge::Left => ABE_LEFT,
        Edge::Right => ABE_RIGHT,
    };
    match edge {
        Edge::Top => r.bottom = r.top + size,
        Edge::Bottom => r.top = r.bottom - size,
        Edge::Left => r.right = r.left + size,
        Edge::Right => r.left = r.right - size,
    }
    d.rc = r;
    set_own_strip(h, Some((edge, r)));
    // SAFETY: reserves the rectangle for our own app bar.
    unsafe { SHAppBarMessage(ABM_SETPOS, &mut d) };
    Ok(())
}

pub fn release_edge(w: RawWindowHandle) -> Result<()> {
    let h = hwnd(w)?;
    let mut d = appbar(h);
    // SAFETY: unregistering our own app bar.
    unsafe { SHAppBarMessage(ABM_REMOVE, &mut d) };
    set_own_strip(h, None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroIsize;

    use raw_window_handle::Win32WindowHandle;
    use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_POPUP};

    use super::*;

    /// 11: display-only windows are not boards for covered points (02 `ignore_covered`).
    #[test]
    fn display_only_windows_are_not_boards() {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        // SAFETY: a hidden window of a system class, destroyed below.
        let h = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_POPUP,
                0,
                0,
                10,
                10,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        let raw = RawWindowHandle::Win32(Win32WindowHandle::new(
            NonZeroIsize::new(h as isize).expect("a window"),
        ));
        let registered = || OVERLAYS.lock().unwrap().contains(&(h as usize));
        let opts = |display_only| OverlayOptions {
            topmost: false,
            no_focus: true,
            display_only,
        };
        configure(raw, opts(false)).unwrap();
        assert!(registered());
        configure(raw, opts(true)).unwrap();
        assert!(!registered());
        // SAFETY: the window created above.
        unsafe { DestroyWindow(h) };
    }
}
