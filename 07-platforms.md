# 07 — Platform Layer

## Trait

All OS access goes through `uca-platform`:

```rust
pub trait Platform: Send + Sync {
    fn capabilities(&self) -> Capabilities;
    fn monitors(&self) -> Vec<MonitorInfo>;
    fn subscribe(&self, tx: Sender<PlatformEvent>);   // MonitorsChanged, ForegroundAppChanged, HighContrastChanged

    // Output
    fn inject_key(&self, key: Key, dir: KeyDir) -> Result<()>;
    fn type_text(&self, text: &str) -> Result<()>;
    fn mouse_move_abs(&self, p: PhysPoint) -> Result<()>;
    fn mouse_move_rel(&self, dx: f64, dy: f64) -> Result<()>;
    fn mouse_button(&self, b: MouseButton, dir: KeyDir) -> Result<()>;
    fn mouse_scroll(&self, dx: f32, dy: f32) -> Result<()>;
    fn cursor_pos(&self) -> Option<PhysPoint>;

    // Global input hooks (switches)
    fn hook_keyboard(&self, cb: HookCallback) -> Result<HookHandle>; // cb returns Consume|Pass; decided: hook events are `Button { trigger, phase, injected }` or, from the mouse hook, `Move { pos, injected }`
    fn hook_mouse(&self, cb: HookCallback) -> Result<HookHandle>;

    // Overlay
    fn configure_overlay(&self, w: &RawWindow, opts: OverlayOptions) -> Result<()>;
    fn reserve_edge(&self, w: &RawWindow, edge: Edge, size_phys: u32) -> Result<()>;

    // System
    fn launch(&self, path: &str, args: &[String]) -> Result<()>;
    fn focus_window(&self, q: WindowQuery) -> Result<()>;
    fn foreground_app(&self) -> Option<String>;
    fn clipboard_chord(&self, op: ClipboardOp) -> Vec<Key>;
    fn ui_locale(&self) -> Option<String>;              // decided: the OS UI locale as BCP-47, default for `ui_language` (17)  // decided: keys for copy/paste/cut (e.g. ctrl+c; macOS meta+c), pressed by the executor on the chosen output
    fn key_label(&self, key: Key, mods: Modifiers) -> Option<String>; // character this key produces under the current OS keyboard layout (`17`)
    // also emits PlatformEvent::KeyboardLayoutChanged

    // Accessibility targets (switch pointer "targets" method, see 04)
    fn ui_targets(&self, scope: TargetScope, max: usize) -> Result<Vec<UiTarget>>;
    fn ui_invoke(&self, target: &UiTargetId) -> Result<()>;   // press/invoke without a mouse click

    fn request_permissions(&self, p: &[Permission]) -> PermissionState; // macOS mainly
}

pub struct Capabilities {
    pub inject_keys: bool, pub inject_text: bool, pub inject_mouse: bool,
    pub read_cursor: bool, pub global_hooks: bool, pub hook_can_consume: bool,
    pub overlay_topmost: bool, pub overlay_no_focus: bool, pub overlay_click_through: bool,
    pub reserve_edge: bool, pub per_monitor_dpi: bool,
    pub foreground_app: bool, pub elevated_injection: bool,
    pub ui_targets: bool, pub ui_invoke: bool,
}

pub struct UiTarget {
    pub id: UiTargetId,              // opaque, valid until next ui_targets() call
    pub rect_phys: RectPhys,         // on-screen bounds; targets with empty/off-screen rects are dropped
    pub role: UiRole,                // Button | Link | MenuItem | Tab | CheckBox | ListItem | Edit | ComboBox | TreeItem | Other
    pub name: Option<String>,        // accessible name, for labels
    pub container: Option<UiTargetId>, // nearest grouping ancestor (toolbar, list, pane), for group scanning
    pub enabled: bool,               // disabled targets are skipped
}
```

`ui_targets` must return within 300 ms (run on its own thread with a timeout; on timeout return what was collected and log it). Results are sorted top-to-bottom, left-to-right.

Key names: platform-neutral enum `Key` modeled on W3C UI Events `code` values (`KeyA`, `Digit1`, `ShiftLeft`, `F13`, `ArrowUp`, …). YAML accepts lowercase aliases (`q`, `shift`, `enter`, `f13`). Each backend maps `Key` → native code.

Decided details:
- **Location and coverage:** `Key` lives in `uca-types`. It covers the full W3C UI Events `KeyboardEvent.code` list: writing-system, functional, control-pad, arrow, numpad, function F1–F24, media, browser and IME/international keys. A backend reports the keys it cannot produce as unsupported.
- **Name matching:** a YAML key name may be the W3C code in any case (`KeyA`, `keya`, `F13`, `f13`) or one of these aliases:
  - `a`–`z` and `0`–`9`
  - `enter`, `esc` / `escape`, `space`, `tab`, `backspace`, `delete`
  - `up` / `down` / `left` / `right`, `home` / `end`, `pageup` / `pagedown`
  - `shift` / `ctrl` / `alt` / `meta`, and `shiftright` / `ctrlright` / `altright` / `metaright`

  An unknown name is an error with a "Did you mean" suggestion.
- **Generic modifier aliases** mean the left key everywhere. `shift` is `ShiftLeft` both for output and as a trigger: `key:shift` matches only ShiftLeft.

**Trait growth (decided):** like config keys and actions, the trait gains methods with the feature that needs them. M1 has output, hooks, monitors, events, overlay, system and `clipboard_chord`. `ui_targets` / `ui_invoke` arrive in M3, `key_label` in M4 and `request_permissions` with macOS.

**Overlay windows (decided):** the overlay methods take a `raw-window-handle` `RawWindowHandle`. The config dir (`02`) comes from `Platform::config_dir()`, so OS paths stay in platform crates.

**Windows backend (decided):**
- **Bindings:** the `windows-sys` crate.
- **Absolute pointer moves:** `SendInput` with `MOUSEEVENTF_MOVE | ABSOLUTE | VIRTUALDESK`, normalised over the virtual desktop and tagged as UCA's own input (SAFETY-07).
- **`app.launch`:** `ShellExecuteW`, so programs, documents, folders and URLs open as they do from Explorer.
- **`window.focus`:** `title_contains` matches visible top-level windows ignoring case, and the topmost match in Z-order is focused. `next` focuses the next window in Alt+Tab order, skipping tool windows and UCA's own windows.

## Capability matrix (target)

| Capability | Windows | Linux X11 | Linux Wayland | macOS |
| --- | --- | --- | --- | --- |
| inject keys/mouse | `SendInput` | XTest | RemoteDesktop portal via libei, else `/dev/uinput` | `CGEventPost` |
| inject text (Unicode) | `SendInput` `KEYEVENTF_UNICODE` | XTest + temporary keysym remap | portal/libei keymap or uinput + keymap; may be limited | `CGEventKeyboardSetUnicodeString` |
| read cursor | `GetCursorPos` | `XQueryPointer` | ✗ (track own injected position; uinput relative) | `CGEventGetLocation` |
| global hooks | `WH_KEYBOARD_LL`, `WH_MOUSE_LL` / Raw Input | XInput2 (cannot consume → use XGrabKey for bound keys) | evdev (user in `input` group), consume via `EVIOCGRAB` + re-emit through uinput | `CGEventTap` (can consume) |
| overlay topmost, no focus | `WS_EX_TOPMOST\|WS_EX_NOACTIVATE\|WS_EX_TOOLWINDOW\|WS_EX_LAYERED` | override-redirect or `_NET_WM_STATE_ABOVE` + `_NET_WM_WINDOW_TYPE_DOCK` | `wlr-layer-shell` (KDE, wlroots); GNOME: no layer-shell → GNOME Shell extension or degraded normal window | `NSPanel` `.nonactivatingPanel`, level `.screenSaver`, `canJoinAllSpaces` |
| click-through regions | `WM_NCHITTEST` → `HTTRANSPARENT` / `WS_EX_TRANSPARENT` toggling | XShape input region | `wl_surface.set_input_region` | `ignoresMouseEvents` toggling per region via hit-testing view |
| reserve edge | AppBar (`SHAppBarMessage`) | `_NET_WM_STRUT_PARTIAL` | layer-shell exclusive zone | ✗ |
| per-monitor DPI | Per-Monitor v2 manifest | Xrandr; scale from `Xft.dpi`/env (best effort) | `wp_fractional_scale_v1` | native backing scale |
| UI targets (switch pointer) | UI Automation (`IUIAutomation`, cache request for bounds/role/name/`IsInvokePatternAvailable`) | AT-SPI2 (`atspi` crate) — quality depends on the toolkit; apps without accessibility → fallback | AT-SPI2 as X11 | `AXUIElement` tree (Accessibility permission) |
| elevated windows | needs `uiAccess=true`, signed, installed in Program Files | n/a | n/a | n/a |
| permissions | none (except uiAccess) | none | portal consent dialog / udev rule for uinput | Accessibility, Input Monitoring, Camera, Screen Recording (if zoom uses screen capture) |
| zoom-to-click capture | Magnification API or DXGI desktop duplication | XGetImage/XShm | screencast portal (PipeWire) | ScreenCaptureKit |

## Windows backend (M1, reference implementation)

- Crate `windows` (windows-rs). DPI awareness set in the app manifest (`PerMonitorV2`).
- Hooks run on a dedicated thread with a message loop; callback must return within 300 ms (Windows unhooks slow hooks) → do no work in the hook beyond a channel send and a consume decision from a precomputed set.
- Injected events must be distinguished from real ones (`LLKHF_INJECTED` / `dwExtraInfo` tag `UCA_MAGIC`) so UCA never reacts to its own output.
- Installer (M4): MSIX or WiX; code signing required for `uiAccess`.

## Linux X11 backend (M5)

- `x11rb`. XTest for injection, XInput2 raw events for hooks, `XGrabKey`/`XGrabButton` for consumed switches.
- Compositor transparency requires ARGB visual; if no compositor, fall back to opaque boards with `opacity` ignored (warn).

## Linux Wayland backend (M6)

- Detect compositor at runtime (`XDG_CURRENT_DESKTOP`, available globals).
- Injection preference: (1) `org.freedesktop.portal.RemoteDesktop` + libei (`reis` crate) — user approves once, persist token; (2) `/dev/uinput` if accessible (document udev rule in installer).
- Overlay: `smithay-client-toolkit` layer-shell where available. GNOME: ship a small GNOME Shell extension providing an always-on-top overlay, else degraded mode with explicit warning.
- Cursor position unavailable: in Pointer mode UCA owns the cursor (absolute moves via libei/uinput absolute device), so position is known.

## macOS backend (M7)

- `objc2` + `objc2-app-kit`, `core-graphics`.
- First-run permission onboarding screen that checks each permission and deep-links to System Settings.
- Notarization required for distribution.

## Acceptance tests

- `Platform` has a `FakePlatform` recording all calls; executor tests assert call sequences.
- Windows integration test (manual/CI on Windows runner): inject text "Hello, Wörld ✓" into Notepad; verify content.
- Self-loop test: UCA's injected keys never appear as triggers.
- Each backend reports `Capabilities` honestly; UI hides features where false (snapshot test of settings UI per capability set).
