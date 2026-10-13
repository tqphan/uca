// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Launching, window focus and the foreground app (07, decided).

use uca_platform::{PlatformError, Result, WindowQuery};
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, TRUE};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess,
    QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow,
    ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_RESTORE, SW_SHOWNORMAL, WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Joins arguments with Windows command-line quoting.
fn command_line(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if !a.is_empty() && !a.contains([' ', '\t', '"']) {
                return a.clone();
            }
            let mut out = String::from('"');
            let mut backslashes = 0;
            for c in a.chars() {
                match c {
                    '\\' => backslashes += 1,
                    '"' => {
                        out.push_str(&"\\".repeat(backslashes * 2 + 1));
                        out.push('"');
                        backslashes = 0;
                    }
                    c => {
                        out.push_str(&"\\".repeat(backslashes));
                        out.push(c);
                        backslashes = 0;
                    }
                }
            }
            out.push_str(&"\\".repeat(backslashes * 2));
            out.push('"');
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn launch(path: &str, args: &[String]) -> Result<()> {
    let verb = wide("open");
    let file = wide(path);
    let params = wide(&command_line(args));
    // SAFETY: all strings are NUL-terminated UTF-16 buffers alive for the call.
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            if args.is_empty() {
                std::ptr::null()
            } else {
                params.as_ptr()
            },
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute returns a value greater than 32 on success.
    if r as isize > 32 {
        Ok(())
    } else {
        Err(PlatformError::Failed {
            what: "app.launch",
            message: format!("ShellExecute error {}", r as isize),
        })
    }
}

fn title(h: HWND) -> String {
    // SAFETY: valid window handle from EnumWindows; buffer sized from the reported length.
    let len = unsafe { GetWindowTextLengthW(h) };
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    // SAFETY: as above.
    let n = unsafe { GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Alt+Tab candidates: visible, not cloaked, not tool windows, unowned or app windows, titled,
/// not UCA's own.
fn is_alt_tab(h: HWND) -> bool {
    // SAFETY: plain window queries on a handle from EnumWindows.
    unsafe {
        if IsWindowVisible(h) == 0 {
            return false;
        }
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW != 0 {
            return false;
        }
        if !GetWindow(h, GW_OWNER).is_null() && ex & WS_EX_APPWINDOW == 0 {
            return false;
        }
        let mut cloaked: u32 = 0;
        let ok =
            DwmGetWindowAttribute(h, DWMWA_CLOAKED as u32, &mut cloaked as *mut _ as *mut _, 4);
        if ok == 0 && cloaked != 0 {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        if pid == GetCurrentProcessId() {
            return false;
        }
    }
    !title(h).is_empty()
}

unsafe extern "system" fn collect(h: HWND, data: LPARAM) -> i32 {
    // SAFETY: `data` is the Vec passed by `windows()` for the duration of the call.
    let list = unsafe { &mut *(data as *mut Vec<HWND>) };
    if is_alt_tab(h) {
        list.push(h);
    }
    TRUE
}

/// Alt+Tab candidates in Z-order, top first.
fn windows() -> Vec<HWND> {
    let mut list: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into `list`, which outlives the call.
    unsafe { EnumWindows(Some(collect), &mut list as *mut _ as LPARAM) };
    list
}

fn bring_to_front(h: HWND) -> Result<()> {
    // SAFETY: plain window calls; input queues are attached only for the call and detached again.
    unsafe {
        if IsIconic(h) != 0 {
            ShowWindow(h, SW_RESTORE);
        }
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let me = GetCurrentThreadId();
        let attached =
            fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, TRUE) != 0;
        let ok = SetForegroundWindow(h) != 0;
        if attached {
            AttachThreadInput(me, fg_thread, 0);
        }
        if ok {
            Ok(())
        } else {
            Err(PlatformError::Failed {
                what: "window.focus",
                message: "Windows refused to change the foreground window".into(),
            })
        }
    }
}

pub fn focus(q: &WindowQuery) -> Result<()> {
    let list = windows();
    let target = match q {
        WindowQuery::TitleContains(t) => {
            let needle = t.to_lowercase();
            list.iter()
                .copied()
                .find(|h| title(*h).to_lowercase().contains(&needle))
        }
        WindowQuery::Next => {
            // SAFETY: plain query.
            let fg = unsafe { GetForegroundWindow() };
            match list.iter().position(|h| *h == fg) {
                Some(i) => list.get(i + 1).or(list.first()).copied(),
                None => list.first().copied(),
            }
        }
    };
    match target {
        Some(h) => bring_to_front(h),
        None => Err(PlatformError::Failed {
            what: "window.focus",
            message: "no matching window".into(),
        }),
    }
}

/// The executable name of the foreground app (11: names only).
pub fn foreground_app() -> Option<String> {
    // SAFETY: handles are checked and closed; the buffer size is passed in and updated.
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len)
                != 0;
        CloseHandle(process);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_quoting() {
        let a = |v: &[&str]| command_line(&v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        assert_eq!(a(&["a", "b"]), "a b");
        assert_eq!(a(&["two words"]), "\"two words\"");
        assert_eq!(a(&["say \"hi\""]), "\"say \\\"hi\\\"\"");
        assert_eq!(a(&["C:\\dir with space\\"]), "\"C:\\dir with space\\\\\"");
        assert_eq!(a(&[""]), "\"\"");
    }
}

/// The Documents folder through the Known Folder API, so a moved folder (for
/// example to OneDrive) is honoured (11).
pub fn documents_dir() -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath};
    let mut path: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: `path` receives a string the shell allocates; it is freed below.
    let hr =
        unsafe { SHGetKnownFolderPath(&FOLDERID_Documents, 0, std::ptr::null_mut(), &mut path) };
    let result = if hr >= 0 && !path.is_null() {
        // SAFETY: a NUL-terminated wide string from the shell.
        let len = (0..).take_while(|&i| unsafe { *path.add(i) } != 0).count();
        // SAFETY: `len` characters were just read from `path`.
        let wide = unsafe { std::slice::from_raw_parts(path, len) };
        Some(std::path::PathBuf::from(std::ffi::OsString::from_wide(
            wide,
        )))
    } else {
        None
    };
    // SAFETY: freeing the shell's allocation (null is allowed).
    unsafe { CoTaskMemFree(path.cast()) };
    result
}

#[link(name = "ntdll")]
extern "system" {
    fn RtlGetVersion(
        info: *mut windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW,
    ) -> i32;
}

/// "Windows <major>.<minor>.<build>" from RtlGetVersion, which, unlike
/// GetVersionEx, is not capped by the app manifest (11).
pub fn os_version() -> Option<String> {
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
    // SAFETY: a zeroed plain-data struct with its size set, as the API needs.
    let mut info: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
    info.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
    // SAFETY: `info` is a valid, sized OSVERSIONINFOW.
    let status = unsafe { RtlGetVersion(&mut info) };
    (status == 0).then(|| {
        format!(
            "Windows {}.{}.{}",
            info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
        )
    })
}

#[cfg(test)]
mod diagnostics_tests {
    #[test]
    fn documents_and_version() {
        let docs = super::documents_dir().expect("a Documents folder");
        assert!(docs.is_absolute(), "{docs:?}");
        let v = super::os_version().expect("a version");
        assert!(
            v.starts_with("Windows 10.") || v.starts_with("Windows 6."),
            "{v}"
        );
    }
}
